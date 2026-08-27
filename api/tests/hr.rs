//! Leave: routing to the direct manager, who may decide, and the overlap rule.

mod common;

use axum::http::StatusCode;
use common::{code, run, uuid, TestApp};
use serde_json::json;

/// A leave request is routed to the requester's direct manager, and the manager sees
/// it in their `pending_for_me` queue.
#[test]
fn a_leave_request_routes_to_the_direct_manager() {
    run(|| async {
        let app = TestApp::start().await;
        let driver = app.token(&app.fx.driver).await;

        let (status, body) = app
            .post(
                "/api/v1/hr/leave/requests",
                &driver,
                json!({
                    "type_key": "annual",
                    "start_date": "2030-09-02",
                    "end_date": "2030-09-06",
                    "reason": "Family visit"
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["status"], "pending");
        let days: f64 = body["days"]
            .as_str()
            .expect("days is a decimal string")
            .parse()
            .expect("a number");
        assert_eq!(days, 5.0, "both ends of the window count");
        assert_eq!(
            body["current_approver_id"].as_str().unwrap(),
            app.fx.dock_supervisor.employee_id.to_string(),
            "the direct manager decides"
        );
        let request_id = uuid(&body["id"]);

        let supervisor = app.token(&app.fx.dock_supervisor).await;
        let (status, queue) = app
            .get("/api/v1/hr/leave/requests?pending_for_me=1", &supervisor)
            .await;
        assert_eq!(status, StatusCode::OK, "{queue}");
        let waiting: Vec<_> = common::ids(&queue);
        assert!(waiting.contains(&request_id), "{queue}");

        // The manager two levels up did not have it routed to them.
        let warehouse = app.token(&app.fx.warehouse_manager).await;
        let (status, queue) = app
            .get("/api/v1/hr/leave/requests?pending_for_me=1", &warehouse)
            .await;
        assert_eq!(status, StatusCode::OK, "{queue}");
        assert!(!common::ids(&queue).contains(&request_id), "{queue}");
    });
}

/// A manager decides for their own people. Somebody else's request is not theirs to
/// touch, and is hidden behind a 404 rather than acknowledged with a 403.
#[test]
fn a_manager_approves_their_own_report_but_not_a_stranger() {
    run(|| async {
        let app = TestApp::start().await;
        let driver = app.token(&app.fx.driver).await;
        let sales_rep = app.token(&app.fx.sales_rep).await;
        let supervisor = app.token(&app.fx.dock_supervisor).await;

        let (status, mine) = app
            .post(
                "/api/v1/hr/leave/requests",
                &driver,
                json!({"type_key": "annual", "start_date": "2030-03-02", "end_date": "2030-03-04"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{mine}");
        let mine_id = uuid(&mine["id"]);

        let (status, theirs) = app
            .post(
                "/api/v1/hr/leave/requests",
                &sales_rep,
                json!({"type_key": "annual", "start_date": "2030-03-02", "end_date": "2030-03-04"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{theirs}");
        let theirs_id = uuid(&theirs["id"]);

        let (status, body) = app
            .post(
                &format!("/api/v1/hr/leave/requests/{mine_id}/approve"),
                &supervisor,
                json!({"note": "Enjoy it"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "approved");
        assert_eq!(
            body["decided_by"].as_str().unwrap(),
            app.fx.dock_supervisor.employee_id.to_string()
        );

        let (status, body) = app
            .post(
                &format!("/api/v1/hr/leave/requests/{theirs_id}/approve"),
                &supervisor,
                json!({}),
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(code(&body), "not_found");

        // The other request is still pending and still routed to its own manager.
        let sales_manager = app.token(&app.fx.sales_manager).await;
        let (status, body) = app
            .get(
                &format!(
                    "/api/v1/hr/leave/requests?employee_id={}",
                    app.fx.sales_rep.employee_id
                ),
                &sales_manager,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(common::items(&body)[0]["status"], "pending");
    });
}

/// The database carries an exclusion constraint for this, and the API turns it into
/// a plain conflict.
#[test]
fn overlapping_leave_for_the_same_employee_is_rejected() {
    run(|| async {
        let app = TestApp::start().await;
        let driver = app.token(&app.fx.driver).await;

        let (status, body) = app
            .post(
                "/api/v1/hr/leave/requests",
                &driver,
                json!({"type_key": "annual", "start_date": "2030-06-01", "end_date": "2030-06-05"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");

        let (status, body) = app
            .post(
                "/api/v1/hr/leave/requests",
                &driver,
                json!({"type_key": "sick", "start_date": "2030-06-05", "end_date": "2030-06-07"}),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(code(&body), "conflict");

        // A window that does not touch the first one is accepted, so the rejection
        // was about the overlap and not about the second request itself.
        let (status, body) = app
            .post(
                "/api/v1/hr/leave/requests",
                &driver,
                json!({"type_key": "sick", "start_date": "2030-06-06", "end_date": "2030-06-07"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    });
}
