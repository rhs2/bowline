//! The shipment state machine and who may move a work order.

mod common;

use axum::http::StatusCode;
use common::{code, run, uuid, TestApp};
use serde_json::json;

/// `draft -> delivered` skips the whole pipeline, so the state machine refuses it
/// with the dedicated `invalid_transition` code.
#[test]
fn an_illegal_shipment_transition_is_refused() {
    run(|| async {
        let app = TestApp::start().await;
        let dispatcher = app.token(&app.fx.dispatcher).await;
        let customer = app.customer(&dispatcher, "T_ACME").await;
        let shipment = app.shipment(&dispatcher, customer).await;

        let (status, body) = app
            .post(
                &format!("/api/v1/ops/shipments/{shipment}/transition"),
                &dispatcher,
                json!({"to": "delivered"}),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(code(&body), "invalid_transition");

        // Nothing moved.
        let (status, detail) = app
            .get(&format!("/api/v1/ops/shipments/{shipment}"), &dispatcher)
            .await;
        assert_eq!(status, StatusCode::OK, "{detail}");
        assert_eq!(detail["status"], "draft");
    });
}

/// The legal chain advances one step at a time and leaves a timeline behind.
#[test]
fn the_legal_chain_from_draft_to_picked_up_succeeds() {
    run(|| async {
        let app = TestApp::start().await;
        let dispatcher = app.token(&app.fx.dispatcher).await;
        let customer = app.customer(&dispatcher, "T_BOREAL").await;
        let shipment = app.shipment(&dispatcher, customer).await;

        for (to, expected_event) in [("booked", "booked"), ("picked_up", "picked_up")] {
            let (status, body) = app
                .post(
                    &format!("/api/v1/ops/shipments/{shipment}/transition"),
                    &dispatcher,
                    json!({"to": to, "location": "Rotterdam"}),
                )
                .await;
            assert_eq!(status, StatusCode::OK, "moving to {to}: {body}");
            assert_eq!(body["status"], to);

            let events: Vec<&str> = body["events"]
                .as_array()
                .expect("a timeline")
                .iter()
                .map(|e| e["event_type"].as_str().unwrap())
                .collect();
            assert!(
                events.contains(&expected_event),
                "the {to} step should leave a {expected_event} event, timeline was {events:?}"
            );
        }

        let (status, detail) = app
            .get(&format!("/api/v1/ops/shipments/{shipment}"), &dispatcher)
            .await;
        assert_eq!(status, StatusCode::OK, "{detail}");
        assert_eq!(detail["status"], "picked_up");
        let allowed: Vec<&str> = detail["allowed_transitions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(allowed, vec!["in_transit", "exception"]);
    });
}

/// A work order belongs to the person holding it and to the managers above them.
/// Anyone else is refused.
#[test]
fn a_work_order_moves_only_for_its_assignee_or_a_manager_above_them() {
    run(|| async {
        let app = TestApp::start().await;
        let dispatcher = app.token(&app.fx.dispatcher).await;
        let customer = app.customer(&dispatcher, "T_CALDERA").await;
        let shipment = app.shipment(&dispatcher, customer).await;

        let (status, body) = app
            .post(
                "/api/v1/ops/work-orders",
                &dispatcher,
                json!({
                    "shipment_id": shipment,
                    "kind": "loading",
                    "title": "Load bay 4",
                    "instructions": "Twelve pallets, tail lift",
                    "assigned_to": app.fx.dock_worker.employee_id
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["status"], "open");
        let work_order = uuid(&body["id"]);

        // A colleague at the same level has no claim on it.
        let driver = app.token(&app.fx.driver).await;
        let (status, body) = app
            .post(
                &format!("/api/v1/ops/work-orders/{work_order}/status"),
                &driver,
                json!({"status": "done"}),
            )
            .await;
        assert!(
            status == StatusCode::FORBIDDEN || status == StatusCode::NOT_FOUND,
            "a bystander must not move the task, got {status}: {body}"
        );
        assert!(matches!(code(&body), "forbidden" | "not_found"), "{body}");

        // The assignee may.
        let dock_worker = app.token(&app.fx.dock_worker).await;
        let (status, body) = app
            .post(
                &format!("/api/v1/ops/work-orders/{work_order}/status"),
                &dock_worker,
                json!({"status": "in_progress"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "in_progress");
        assert!(!body["started_at"].is_null(), "{body}");

        // So may their supervisor.
        let supervisor = app.token(&app.fx.dock_supervisor).await;
        let (status, body) = app
            .post(
                &format!("/api/v1/ops/work-orders/{work_order}/status"),
                &supervisor,
                json!({"status": "done", "notes": "Loaded and sealed"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "done");
        assert!(!body["completed_at"].is_null(), "{body}");
    });
}
