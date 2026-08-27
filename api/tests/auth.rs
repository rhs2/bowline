//! Login, lockout, refresh rotation with reuse detection, and `/auth/me`.

mod common;

use axum::http::StatusCode;
use common::{code, run, TestApp, LOGIN_MAX_FAILURES, PASSWORD};
use serde_json::json;

/// A correct password returns tokens, and the access token opens an authenticated
/// route.
#[test]
fn login_returns_an_access_token_that_opens_the_api() {
    run(|| async {
        let app = TestApp::start().await;
        let (status, body) = app.login(&app.fx.driver.email, PASSWORD).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["token_type"], "Bearer");
        assert_eq!(body["must_change_password"], false);
        assert!(body["expires_in"].as_u64().unwrap_or(0) > 0);

        let token = body["access_token"].as_str().expect("an access token");
        let (status, me) = app.get("/api/v1/auth/me", token).await;
        assert_eq!(status, StatusCode::OK, "{me}");
        assert_eq!(me["user"]["email"], app.fx.driver.email.as_str());
        assert_eq!(
            me["employee"]["id"].as_str().unwrap(),
            app.fx.driver.employee_id.to_string()
        );
    });
}

/// The wrong password is a 401 problem document, and no token comes back.
#[test]
fn a_wrong_password_is_unauthorized() {
    run(|| async {
        let app = TestApp::start().await;
        let (status, body) = app.login(&app.fx.driver.email, "not-the-password-1").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        assert_eq!(code(&body), "unauthorized");
        assert!(body["access_token"].is_null());
    });
}

/// After `LOGIN_MAX_FAILURES` bad attempts the account locks, and even the correct
/// password is answered with 423 until the lockout expires.
#[test]
fn the_account_locks_after_the_configured_number_of_failures() {
    run(|| async {
        let app = TestApp::start().await;
        let email = app.fx.dock_worker.email.clone();

        for attempt in 1..LOGIN_MAX_FAILURES {
            let (status, body) = app.login(&email, "wrong-password-1").await;
            assert_eq!(
                status,
                StatusCode::UNAUTHORIZED,
                "attempt {attempt} should still be a plain rejection: {body}"
            );
        }

        let (status, body) = app.login(&email, "wrong-password-1").await;
        assert_eq!(status, StatusCode::LOCKED, "{body}");
        assert_eq!(code(&body), "locked");

        let (status, body) = app.login(&email, PASSWORD).await;
        assert_eq!(
            status,
            StatusCode::LOCKED,
            "a locked account must stay shut even for the right password: {body}"
        );
        assert_eq!(code(&body), "locked");
    });
}

/// Refreshing hands out a new pair and retires the token that was presented.
#[test]
fn refreshing_rotates_the_token_and_retires_the_old_one() {
    run(|| async {
        let app = TestApp::start().await;
        let (_, first_refresh) = app.session(&app.fx.accountant).await;

        let (status, body) = app
            .post_anon(
                "/api/v1/auth/refresh",
                json!({"refresh_token": first_refresh}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let second_access = body["access_token"].as_str().expect("access").to_string();
        let second_refresh = body["refresh_token"].as_str().expect("refresh").to_string();
        assert_ne!(second_refresh, first_refresh, "the token must rotate");

        let (status, me) = app.get("/api/v1/auth/me", &second_access).await;
        assert_eq!(status, StatusCode::OK, "the new access token works: {me}");

        let (status, body) = app
            .post_anon(
                "/api/v1/auth/refresh",
                json!({"refresh_token": first_refresh}),
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
        assert_eq!(code(&body), "unauthorized");
    });
}

/// Replaying a refresh token that has already been spent means the family leaked, so
/// every token in it is revoked, including the one that is still current.
#[test]
fn replaying_a_used_refresh_token_revokes_the_whole_family() {
    run(|| async {
        let app = TestApp::start().await;
        let (_, first_refresh) = app.session(&app.fx.accountant).await;

        let (status, body) = app
            .post_anon(
                "/api/v1/auth/refresh",
                json!({"refresh_token": first_refresh}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let live_refresh = body["refresh_token"].as_str().expect("refresh").to_string();

        // The replay is what trips the detector.
        let (status, body) = app
            .post_anon(
                "/api/v1/auth/refresh",
                json!({"refresh_token": first_refresh}),
            )
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");

        // The token that was still good before the replay is now dead too.
        let (status, body) = app
            .post_anon(
                "/api/v1/auth/refresh",
                json!({"refresh_token": live_refresh}),
            )
            .await;
        assert_eq!(
            status,
            StatusCode::UNAUTHORIZED,
            "reuse must revoke the whole family: {body}"
        );
        assert_eq!(code(&body), "unauthorized");
    });
}

/// `/auth/me` is where a client learns what it may do and who it reports to.
#[test]
fn me_returns_the_callers_permissions_and_chain_of_command() {
    run(|| async {
        let app = TestApp::start().await;
        let token = app.token(&app.fx.dock_worker).await;
        let (status, me) = app.get("/api/v1/auth/me", &token).await;
        assert_eq!(status, StatusCode::OK, "{me}");

        let roles: Vec<&str> = me["roles"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r.as_str().unwrap())
            .collect();
        assert!(roles.contains(&"field_worker"), "roles were {roles:?}");
        assert!(roles.contains(&"baseline"), "roles were {roles:?}");

        let permissions: Vec<&str> = me["permissions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p.as_str().unwrap())
            .collect();
        for expected in [
            "employees:read:self",
            "tasks:read:self",
            "tasks:update:self",
            "leave:request",
            "tickets:create",
            "messages:send:chain",
        ] {
            assert!(
                permissions.contains(&expected),
                "{expected} is missing from {permissions:?}"
            );
        }
        assert!(
            !permissions.contains(&"employees:read:all"),
            "a dock worker must not hold a company wide read"
        );

        // Nearest manager first, ending at the chief executive.
        let chain: Vec<String> = me["chain"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            chain,
            vec![
                app.fx.dock_supervisor.employee_id.to_string(),
                app.fx.warehouse_manager.employee_id.to_string(),
                app.fx.ceo.employee_id.to_string(),
            ]
        );
    });
}
