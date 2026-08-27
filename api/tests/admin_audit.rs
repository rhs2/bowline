//! The audit trail behind every mutation, and what a password reset does to the
//! tokens already in circulation.

mod common;

use axum::http::StatusCode;
use common::{code, items, run, TestApp};
use serde_json::json;

/// A change through the API leaves one audit row carrying the actor and the before
/// and after images of the row.
#[test]
fn a_mutation_writes_an_audit_row_with_the_actor_and_both_images() {
    run(|| async {
        let app = TestApp::start().await;
        let ceo = app.token(&app.fx.ceo).await;
        let it_admin = app.token(&app.fx.it_admin).await;

        let (status, body) = app
            .patch(
                &format!("/api/v1/employees/{}", app.fx.driver.employee_id),
                &ceo,
                json!({"phone": "+1 555 0101", "site": "Sea-port depot"}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["phone"], "+1 555 0101");

        let (status, log) = app
            .get(
                &format!(
                    "/api/v1/admin/audit?entity_type=employee&entity_id={}",
                    app.fx.driver.employee_id
                ),
                &it_admin,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{log}");
        let entry = items(&log)
            .iter()
            .find(|e| e["action"] == "employee.update")
            .expect("the update was recorded");

        assert_eq!(
            entry["actor_employee_id"].as_str().unwrap(),
            app.fx.ceo.employee_id.to_string(),
            "the audit row names who did it"
        );
        assert_eq!(entry["actor_name"], "Ada Kestrel");
        assert!(entry["before"]["phone"].is_null(), "{entry}");
        assert_eq!(entry["after"]["phone"], "+1 555 0101", "{entry}");
        assert_eq!(entry["before"]["site"], "Head Office", "{entry}");
        assert_eq!(entry["after"]["site"], "Sea-port depot", "{entry}");
        assert!(
            entry["request_id"].as_str().is_some(),
            "the row carries the request id: {entry}"
        );
    });
}

/// `audit:read` is what opens the log, and nothing else does.
#[test]
fn the_audit_log_is_readable_only_with_audit_read() {
    run(|| async {
        let app = TestApp::start().await;

        // The executive and the platform administrator both hold audit:read.
        for holder in [&app.fx.ceo, &app.fx.it_admin] {
            let token = app.token(holder).await;
            let (status, body) = app.get("/api/v1/admin/audit", &token).await;
            assert_eq!(
                status,
                StatusCode::OK,
                "{} was refused: {body}",
                holder.email
            );
        }

        // The dock supervisor and the accountant do not.
        for outsider in [&app.fx.dock_supervisor, &app.fx.accountant] {
            let token = app.token(outsider).await;
            let (status, body) = app.get("/api/v1/admin/audit", &token).await;
            assert_eq!(
                status,
                StatusCode::FORBIDDEN,
                "{} should be refused: {body}",
                outsider.email
            );
            assert_eq!(code(&body), "forbidden");
        }
    });
}

/// Resetting a password bumps the token version, so every access token minted before
/// the reset stops working straight away.
#[test]
fn resetting_a_password_invalidates_the_existing_access_tokens() {
    run(|| async {
        let app = TestApp::start().await;
        let it_admin = app.token(&app.fx.it_admin).await;
        let (driver_access, driver_refresh) = app.session(&app.fx.driver).await;

        // The token works before the reset.
        let (status, body) = app.get("/api/v1/auth/me", &driver_access).await;
        assert_eq!(status, StatusCode::OK, "{body}");

        let (status, body) = app
            .post_empty(
                &format!(
                    "/api/v1/admin/users/{}/reset-password",
                    app.fx.driver.user_id
                ),
                &it_admin,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let temporary = body["temporary_password"]
            .as_str()
            .expect("a one-time password")
            .to_string();
        assert_eq!(body["user"]["must_change_password"], true);

        // The same access token is now refused.
        let (status, body) = app.get("/api/v1/auth/me", &driver_access).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        assert_eq!(code(&body), "unauthorized");

        // So is the refresh token that came with it.
        let (status, body) = app
            .post_anon(
                "/api/v1/auth/refresh",
                json!({"refresh_token": driver_refresh}),
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

        // The temporary password does work, and the session it opens is still held
        // at the door until the password is changed.
        let (status, body) = app.login(&app.fx.driver.email, &temporary).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["must_change_password"], true);
        let fresh = body["access_token"].as_str().unwrap().to_string();
        let (status, body) = app.get("/api/v1/employees", &fresh).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(code(&body), "forbidden");
    });
}
