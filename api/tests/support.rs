//! The service desk: the SLA clock, triage by an agent, and the lifecycle.

mod common;

use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use common::{code, run, uuid, TestApp};
use serde_json::{json, Value};

fn timestamp(value: &Value) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(value.as_str().expect("a timestamp"))
        .expect("an RFC 3339 timestamp")
        .with_timezone(&Utc)
}

async fn open_ticket(app: &TestApp, token: &str, priority: &str) -> Value {
    let (status, body) = app
        .post(
            "/api/v1/support/tickets",
            token,
            json!({
                "category": "it",
                "priority": priority,
                "subject": "Handheld scanner will not pair",
                "body": "The scanner on bay 4 stopped pairing after this morning's update."
            }),
        )
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    body
}

/// The first response target comes from the priority: one hour for urgent, four for
/// high, a day for normal and three for low.
#[test]
fn creating_a_ticket_sets_an_sla_derived_from_its_priority() {
    run(|| async {
        let app = TestApp::start().await;
        let dock_worker = app.token(&app.fx.dock_worker).await;

        for (priority, hours) in [("urgent", 1), ("high", 4), ("normal", 24), ("low", 72)] {
            let ticket = open_ticket(&app, &dock_worker, priority).await;
            assert_eq!(ticket["priority"], priority);
            assert_eq!(ticket["status"], "open");
            assert!(ticket["ticket_no"].as_str().unwrap().starts_with("TKT-"));

            let promised = timestamp(&ticket["sla_due_at"]) - timestamp(&ticket["created_at"]);
            // The window is generous enough to absorb clock skew between this
            // process and the database, and still far narrower than the gap between
            // any two of the four targets.
            let minutes = promised.num_minutes();
            assert!(
                (hours * 60 - 5..=hours * 60 + 5).contains(&minutes),
                "a {priority} ticket promises {hours}h, the clock said {minutes} minutes"
            );
            assert_eq!(ticket["sla_breached"], false, "{ticket}");
        }
    });
}

/// An agent triages by taking the ticket, works it, and resolves it. The requester
/// then closes it themselves.
#[test]
fn an_agent_assigns_and_resolves_and_the_requester_closes() {
    run(|| async {
        let app = TestApp::start().await;
        let dock_worker = app.token(&app.fx.dock_worker).await;
        let agent = app.token(&app.fx.support_agent).await;
        let ticket = open_ticket(&app, &dock_worker, "high").await;
        let id = uuid(&ticket["id"]);

        let (status, body) = app
            .post(
                &format!("/api/v1/support/tickets/{id}/assign"),
                &agent,
                json!({"assignee_id": app.fx.support_agent.employee_id}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "triaged", "assignment is the triage step");
        assert_eq!(
            body["assignee_id"].as_str().unwrap(),
            app.fx.support_agent.employee_id.to_string()
        );

        // The desk's first reply stops the SLA clock.
        let (status, body) = app
            .post(
                &format!("/api/v1/support/tickets/{id}/messages"),
                &agent,
                json!({"body": "Looking at it now, please leave the scanner on the cradle."}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let (status, detail) = app
            .get(&format!("/api/v1/support/tickets/{id}"), &agent)
            .await;
        assert_eq!(status, StatusCode::OK, "{detail}");
        assert!(!detail["first_response_at"].is_null(), "{detail}");

        for next in ["in_progress", "resolved"] {
            let (status, body) = app
                .post(
                    &format!("/api/v1/support/tickets/{id}/status"),
                    &agent,
                    json!({"status": next}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "moving to {next}: {body}");
            assert_eq!(body["status"], next);
        }

        // A requester may not push a ticket around, but they may close their own.
        let (status, body) = app
            .post(
                &format!("/api/v1/support/tickets/{id}/status"),
                &dock_worker,
                json!({"status": "closed"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "closed");
        assert!(!body["closed_at"].is_null(), "{body}");

        let (status, body) = app
            .post(
                &format!("/api/v1/support/tickets/{id}/rate"),
                &dock_worker,
                json!({"satisfaction": 5}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["satisfaction"], 5);
    });
}

/// The lifecycle only moves one step at a time, and a requester cannot drive it.
#[test]
fn an_illegal_ticket_lifecycle_jump_is_rejected() {
    run(|| async {
        let app = TestApp::start().await;
        let dock_worker = app.token(&app.fx.dock_worker).await;
        let agent = app.token(&app.fx.support_agent).await;
        let ticket = open_ticket(&app, &dock_worker, "normal").await;
        let id = uuid(&ticket["id"]);

        // open -> resolved skips triage and work.
        let (status, body) = app
            .post(
                &format!("/api/v1/support/tickets/{id}/status"),
                &agent,
                json!({"status": "resolved"}),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(code(&body), "invalid_transition");

        // Triage without an owner is refused too.
        let (status, body) = app
            .post(
                &format!("/api/v1/support/tickets/{id}/status"),
                &agent,
                json!({"status": "triaged"}),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(code(&body), "conflict");

        // And the requester may not triage their own ticket even when the step is
        // otherwise legal.
        app.post(
            &format!("/api/v1/support/tickets/{id}/assign"),
            &agent,
            json!({"assignee_id": app.fx.support_agent.employee_id}),
        )
        .await;
        let (status, body) = app
            .post(
                &format!("/api/v1/support/tickets/{id}/status"),
                &dock_worker,
                json!({"status": "in_progress"}),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(code(&body), "forbidden");
    });
}
