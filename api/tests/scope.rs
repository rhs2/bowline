//! The four hierarchy scopes, seen through `/employees`.

mod common;

use axum::http::StatusCode;
use common::{code, ids, items, run, total, TestApp};

/// `employees:read:subtree` stops at the edge of the caller's own branch: the
/// supervisor sees their dock crew and nobody from the commercial subtree.
#[test]
fn a_supervisor_lists_their_own_subtree_and_not_the_other_department() {
    run(|| async {
        let app = TestApp::start().await;
        let token = app.token(&app.fx.dock_supervisor).await;
        let (status, body) = app.get("/api/v1/employees?per_page=100", &token).await;
        assert_eq!(status, StatusCode::OK, "{body}");

        let visible = ids(&body);
        assert_eq!(total(&body), 3, "supervisor plus two reports: {body}");
        for expected in [
            app.fx.dock_supervisor.employee_id,
            app.fx.driver.employee_id,
            app.fx.dock_worker.employee_id,
        ] {
            assert!(visible.contains(&expected), "{expected} should be visible");
        }
        for hidden in [
            app.fx.sales_manager.employee_id,
            app.fx.sales_rep.employee_id,
            app.fx.warehouse_manager.employee_id,
            app.fx.ceo.employee_id,
        ] {
            assert!(
                !visible.contains(&hidden),
                "{hidden} is outside the supervisor's subtree"
            );
        }
    });
}

/// `employees:read:all` applies no filter at all.
#[test]
fn an_executive_lists_everyone() {
    run(|| async {
        let app = TestApp::start().await;
        let token = app.token(&app.fx.ceo).await;
        let (status, body) = app.get("/api/v1/employees?per_page=100", &token).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(total(&body), app.fx.headcount() as i64);

        let visible = ids(&body);
        for person in app.fx.everyone() {
            assert!(
                visible.contains(&person.employee_id),
                "{} is missing from the executive view",
                person.email
            );
        }
    });
}

/// A field worker holds only `employees:read:self`.
#[test]
fn a_field_worker_lists_only_themselves() {
    run(|| async {
        let app = TestApp::start().await;
        let token = app.token(&app.fx.driver).await;
        let (status, body) = app.get("/api/v1/employees?per_page=100", &token).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(total(&body), 1, "{body}");
        assert_eq!(items(&body).len(), 1);
        assert_eq!(ids(&body), vec![app.fx.driver.employee_id]);
    });
}

/// `GET /employees/{id}/reports` is scoped, exactly like the detail route.
///
/// This once leaked: visibility also accepted `org:read`, which every active user
/// holds through the baseline role, so the scope check never bit and any employee
/// could read the full summary (work email, phone, site, hire date) of anyone's
/// direct reports. The org chart is served by `/org/tree` instead, which carries
/// names, titles and reporting lines only.
#[test]
fn direct_reports_outside_the_callers_scope_are_not_found() {
    run(|| async {
        let app = TestApp::start().await;
        let token = app.token(&app.fx.driver).await;

        let (status, body) = app
            .get(
                &format!(
                    "/api/v1/employees/{}/reports",
                    app.fx.sales_manager.employee_id
                ),
                &token,
            )
            .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "a field worker should not be able to enumerate another department's \
             reporting line, but the route answered: {body}"
        );
    });
}

/// Detail routes hide rows outside the caller's scope behind a 404 rather than a
/// 403, so a caller cannot probe for people they may not see.
#[test]
fn a_record_outside_the_callers_scope_is_not_found_rather_than_forbidden() {
    run(|| async {
        let app = TestApp::start().await;
        let token = app.token(&app.fx.dock_supervisor).await;

        let (status, body) = app
            .get(
                &format!("/api/v1/employees/{}", app.fx.sales_rep.employee_id),
                &token,
            )
            .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
        assert_eq!(code(&body), "not_found");

        // The same route for someone inside the subtree answers normally, so the 404
        // above is about scope and not about a broken route.
        let (status, body) = app
            .get(
                &format!("/api/v1/employees/{}", app.fx.driver.employee_id),
                &token,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["id"].as_str().unwrap(),
            app.fx.driver.employee_id.to_string()
        );
    });
}
