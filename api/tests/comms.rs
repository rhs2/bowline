//! Who may write to whom, company announcements, and the notification outbox.

mod common;

use axum::http::StatusCode;
use common::{code, items, run, uuid, TestApp};
use serde_json::json;

/// `messages:send:chain` reaches the caller's own manager.
#[test]
fn a_dock_worker_may_open_a_thread_with_their_own_manager() {
    run(|| async {
        let app = TestApp::start().await;
        let dock_worker = app.token(&app.fx.dock_worker).await;

        let (status, body) = app
            .post(
                "/api/v1/comms/threads",
                &dock_worker,
                json!({
                    "recipient_ids": [app.fx.dock_supervisor.employee_id],
                    "subject": "Bay 4 pallet jack",
                    "body": "The pallet jack on bay 4 is leaking, can we swap it out?"
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["kind"], "direct");
        let participants: Vec<_> = body["participants"]
            .as_array()
            .expect("participants")
            .iter()
            .map(|p| uuid(&p["employee_id"]))
            .collect();
        assert!(participants.contains(&app.fx.dock_supervisor.employee_id));
        assert!(participants.contains(&app.fx.dock_worker.employee_id));

        // The manager finds it in their inbox.
        let supervisor = app.token(&app.fx.dock_supervisor).await;
        let (status, inbox) = app.get("/api/v1/comms/threads", &supervisor).await;
        assert_eq!(status, StatusCode::OK, "{inbox}");
        let thread = items(&inbox)
            .iter()
            .find(|t| t["subject"] == "Bay 4 pallet jack")
            .expect("the thread reached the manager's inbox");
        assert_eq!(thread["unread_count"], 1, "{thread}");
    });
}

/// The chief financial officer is neither in the dock worker's chain nor in their
/// department, and a field worker holds nothing wider.
#[test]
fn a_dock_worker_may_not_open_a_thread_with_the_cfo() {
    run(|| async {
        let app = TestApp::start().await;
        let dock_worker = app.token(&app.fx.dock_worker).await;

        let (status, body) = app
            .post(
                "/api/v1/comms/threads",
                &dock_worker,
                json!({
                    "recipient_ids": [app.fx.cfo.employee_id],
                    "subject": "About my pay",
                    "body": "Could we talk about the overtime rate?"
                }),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(code(&body), "forbidden");

        // The recipient picker agrees: the CFO is simply not on the list.
        let (status, picker) = app
            .get("/api/v1/comms/recipients?per_page=100", &dock_worker)
            .await;
        assert_eq!(status, StatusCode::OK, "{picker}");
        let reachable: Vec<_> = items(&picker).iter().map(|r| uuid(&r["id"])).collect();
        assert!(!reachable.contains(&app.fx.cfo.employee_id), "{picker}");
        assert!(
            reachable.contains(&app.fx.dock_supervisor.employee_id),
            "{picker}"
        );
    });
}

/// `messages:send:any` needs no relationship at all.
#[test]
fn an_executive_may_message_anyone() {
    run(|| async {
        let app = TestApp::start().await;
        let ceo = app.token(&app.fx.ceo).await;

        let (status, body) = app
            .post(
                "/api/v1/comms/threads",
                &ceo,
                json!({
                    "recipient_ids": [app.fx.sales_rep.employee_id, app.fx.dock_worker.employee_id],
                    "subject": "Thank you",
                    "body": "Good quarter, both of you."
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["participant_count"], 3, "{body}");
    });
}

/// A company-wide announcement needs `messages:broadcast:company`, reaches everyone
/// else in the company, and lands in their inboxes.
#[test]
fn a_company_announcement_needs_the_broadcast_permission_and_reaches_every_inbox() {
    run(|| async {
        let app = TestApp::start().await;
        let announcement = json!({
            "scope": "company",
            "subject": "Winter peak plan",
            "body": "Extra shifts on the sea-port depot from Monday."
        });

        // A manager holds only messages:broadcast:subtree.
        let warehouse = app.token(&app.fx.warehouse_manager).await;
        let (status, body) = app
            .post(
                "/api/v1/comms/announcements",
                &warehouse,
                announcement.clone(),
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(code(&body), "forbidden");

        let ceo = app.token(&app.fx.ceo).await;
        let (status, body) = app
            .post("/api/v1/comms/announcements", &ceo, announcement)
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["kind"], "announcement");
        assert_eq!(
            body["audience_size"].as_i64().unwrap(),
            app.fx.headcount() as i64 - 1,
            "everyone but the sender: {body}"
        );
        assert_eq!(body["notifications"], body["audience_size"]);

        // Someone at the very bottom of the company has it in their inbox.
        let dock_worker = app.token(&app.fx.dock_worker).await;
        let (status, inbox) = app
            .get("/api/v1/comms/threads?kind=announcement", &dock_worker)
            .await;
        assert_eq!(status, StatusCode::OK, "{inbox}");
        let subjects: Vec<&str> = items(&inbox)
            .iter()
            .map(|t| t["subject"].as_str().unwrap())
            .collect();
        assert_eq!(subjects, vec!["Winter peak plan"], "{inbox}");
    });
}

/// Every message written also queues one email per recipient in the outbox, in the
/// same transaction as the message itself.
#[test]
fn sending_a_message_writes_notification_outbox_rows() {
    run(|| async {
        let app = TestApp::start().await;
        let dock_worker = app.token(&app.fx.dock_worker).await;
        assert_eq!(
            app.notifications_for(app.fx.dock_supervisor.employee_id)
                .await,
            0,
            "the outbox starts empty"
        );

        let (status, body) = app
            .post(
                "/api/v1/comms/threads",
                &dock_worker,
                json!({
                    "recipient_ids": [app.fx.dock_supervisor.employee_id],
                    "subject": "Shift swap",
                    "body": "Could I move to the late shift on Thursday?"
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let thread = uuid(&body["id"]);
        assert_eq!(
            app.notifications_for(app.fx.dock_supervisor.employee_id)
                .await,
            1,
            "opening the thread queued one email"
        );
        assert_eq!(
            app.notifications_for(app.fx.dock_worker.employee_id).await,
            0,
            "the sender does not get notified about their own message"
        );

        let (status, body) = app
            .post(
                &format!("/api/v1/comms/threads/{thread}/messages"),
                &dock_worker,
                json!({"body": "Any news on the swap?"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(
            app.notifications_for(app.fx.dock_supervisor.employee_id)
                .await,
            2,
            "the follow-up queued another"
        );

        let subject: String = sqlx::query_scalar(
            "select subject from notifications where recipient_id = $1 order by created_at limit 1",
        )
        .bind(app.fx.dock_supervisor.employee_id)
        .fetch_one(&app.pool)
        .await
        .expect("reading the queued email");
        assert_eq!(subject, "Shift swap");
    });
}
