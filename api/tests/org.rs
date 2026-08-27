//! Re-parenting an employee, and the cycle the reporting line may never form.

mod common;

use axum::http::StatusCode;
use common::{code, ids, run, total, TestApp};
use serde_json::json;

/// Moving a manager moves everyone under them: the ltree path is rewritten for the
/// whole subtree, which is visible in what the new and old parents can list.
#[test]
fn re_parenting_an_employee_rewrites_the_whole_subtree() {
    run(|| async {
        let app = TestApp::start().await;
        let ceo = app.token(&app.fx.ceo).await;

        // Before the move the commercial branch is two people and the warehouse
        // branch is three.
        let sales_manager_token = app.token(&app.fx.sales_manager).await;
        let (_, before) = app
            .get("/api/v1/employees?per_page=100", &sales_manager_token)
            .await;
        assert_eq!(total(&before), 2, "{before}");

        let (status, body) = app
            .patch(
                &format!("/api/v1/employees/{}", app.fx.dock_supervisor.employee_id),
                &ceo,
                json!({"manager_id": app.fx.sales_manager.employee_id}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["manager_id"].as_str().unwrap(),
            app.fx.sales_manager.employee_id.to_string()
        );

        // The supervisor and both of their reports now sit under the sales manager.
        let (status, after) = app
            .get("/api/v1/employees?per_page=100", &sales_manager_token)
            .await;
        assert_eq!(status, StatusCode::OK, "{after}");
        assert_eq!(total(&after), 5, "the subtree followed its head: {after}");
        let visible = ids(&after);
        for expected in [
            app.fx.dock_supervisor.employee_id,
            app.fx.driver.employee_id,
            app.fx.dock_worker.employee_id,
        ] {
            assert!(visible.contains(&expected), "{expected} did not move");
        }

        // And the warehouse manager is left on their own.
        let warehouse_token = app.token(&app.fx.warehouse_manager).await;
        let (_, warehouse) = app
            .get("/api/v1/employees?per_page=100", &warehouse_token)
            .await;
        assert_eq!(total(&warehouse), 1, "{warehouse}");
        assert_eq!(ids(&warehouse), vec![app.fx.warehouse_manager.employee_id]);

        // The materialised path itself was rewritten, two levels down.
        let sales_path = app.path_of(app.fx.sales_manager.employee_id).await;
        let driver_path = app.path_of(app.fx.driver.employee_id).await;
        assert!(
            driver_path.starts_with(&format!("{sales_path}.")),
            "{driver_path} should sit under {sales_path}"
        );
    });
}

/// A manager may not be moved under one of their own reports.
#[test]
fn a_reporting_cycle_is_rejected() {
    run(|| async {
        let app = TestApp::start().await;
        let ceo = app.token(&app.fx.ceo).await;

        let (status, body) = app
            .patch(
                &format!("/api/v1/employees/{}", app.fx.warehouse_manager.employee_id),
                &ceo,
                json!({"manager_id": app.fx.driver.employee_id}),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(code(&body), "conflict");

        // The reporting line is untouched.
        let manager_path = app.path_of(app.fx.warehouse_manager.employee_id).await;
        let driver_path = app.path_of(app.fx.driver.employee_id).await;
        assert!(driver_path.starts_with(&format!("{manager_path}.")));
    });
}
