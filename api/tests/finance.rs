//! The ledger: balanced postings, the closed period lock, overpayment, and who may
//! read the reports.

mod common;

use axum::http::StatusCode;
use common::{code, items, run, uuid, TestApp};
use serde_json::{json, Value};

/// Sum of a decimal-string field across the rows of a report.
fn sum(rows: &[Value], field: &str) -> f64 {
    rows.iter()
        .map(|row| {
            row[field]
                .as_str()
                .expect("a decimal string")
                .parse::<f64>()
                .expect("a number")
        })
        .sum()
}

fn amount(value: &Value) -> f64 {
    value
        .as_str()
        .expect("a decimal string")
        .parse()
        .expect("a number")
}

/// Draft, submit, issue. Issuing writes the AR and revenue legs of one entry, and
/// the ledger as a whole still nets to nothing.
#[test]
fn issuing_an_invoice_posts_a_balanced_entry_and_the_trial_balance_still_sums_to_zero() {
    run(|| async {
        let app = TestApp::start().await;
        let dispatcher = app.token(&app.fx.dispatcher).await;
        let accountant = app.token(&app.fx.accountant).await;
        let customer = app.customer(&dispatcher, "T_LEDGER").await;

        let (status, invoice) = app
            .post(
                "/api/v1/finance/invoices",
                &accountant,
                json!({
                    "customer_id": customer,
                    "due_days": 30,
                    "lines": [
                        {"description": "Sea freight Rotterdam to Hamburg", "quantity": "1",
                         "unit_price": "1000.00", "tax_rate": "0"}
                    ]
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{invoice}");
        assert_eq!(invoice["status"], "draft");
        assert_eq!(amount(&invoice["total"]), 1000.0);
        let invoice_id = uuid(&invoice["id"]);

        let (status, body) = app
            .post_empty(
                &format!("/api/v1/finance/invoices/{invoice_id}/submit"),
                &accountant,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body["status"], "approved",
            "below the approval threshold it goes straight through"
        );

        let (status, issued) = app
            .post(
                &format!("/api/v1/finance/invoices/{invoice_id}/issue"),
                &accountant,
                json!({"issue_date": chrono::Utc::now().date_naive().to_string()}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{issued}");
        assert_eq!(issued["status"], "issued");
        assert!(!issued["journal_entry_id"].is_null(), "{issued}");

        // One entry, sourced from the invoice, debiting receivables and crediting
        // revenue for the same amount.
        let (status, journal) = app
            .get(
                &format!("/api/v1/finance/journal?source_type=invoice&source_id={invoice_id}"),
                &accountant,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{journal}");
        let entries = items(&journal);
        assert_eq!(entries.len(), 1, "{journal}");
        let lines = entries[0]["lines"].as_array().expect("lines");
        assert_eq!(lines.len(), 2, "{journal}");
        assert_eq!(sum(lines, "debit"), sum(lines, "credit"));
        let receivable = lines
            .iter()
            .find(|l| l["account_code"] == "1100")
            .expect("an accounts receivable line");
        assert_eq!(amount(&receivable["debit"]), 1000.0);
        let revenue = lines
            .iter()
            .find(|l| l["account_code"] == "4000")
            .expect("a freight revenue line");
        assert_eq!(amount(&revenue["credit"]), 1000.0);

        let (status, report) = app
            .get("/api/v1/finance/reports/trial-balance", &accountant)
            .await;
        assert_eq!(status, StatusCode::OK, "{report}");
        assert_eq!(report["balanced"], true, "{report}");
        assert_eq!(
            amount(&report["total_debit"]),
            amount(&report["total_credit"])
        );
        let rows = report["rows"].as_array().expect("rows");
        assert_eq!(sum(rows, "balance"), 0.0, "the ledger nets to zero");
        assert!(sum(rows, "debit") > 0.0, "something was actually posted");
    });
}

/// The Rust side refuses an entry whose sides differ before it ever reaches the
/// deferred constraint trigger, and reports it as a field error.
#[test]
fn an_unbalanced_manual_entry_is_rejected() {
    run(|| async {
        let app = TestApp::start().await;
        let accountant = app.token(&app.fx.accountant).await;
        let today = chrono::Utc::now().date_naive().to_string();

        let (status, body) = app
            .post(
                "/api/v1/finance/journal",
                &accountant,
                json!({
                    "entry_date": today,
                    "memo": "Deliberately lopsided",
                    "lines": [
                        {"account_code": "1000", "debit": "100.00", "credit": "0"},
                        {"account_code": "4000", "debit": "0", "credit": "90.00"}
                    ]
                }),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(code(&body), "validation_failed");
        let fields: Vec<&str> = body["errors"]
            .as_array()
            .expect("field errors")
            .iter()
            .map(|e| e["field"].as_str().unwrap())
            .collect();
        assert!(fields.contains(&"lines"), "{body}");

        // Nothing was written.
        let (status, journal) = app
            .get("/api/v1/finance/journal?source_type=manual", &accountant)
            .await;
        assert_eq!(status, StatusCode::OK, "{journal}");
        assert!(items(&journal).is_empty(), "{journal}");
    });
}

/// A payment may settle an invoice but never exceed what is owed.
#[test]
fn overpaying_an_invoice_is_rejected() {
    run(|| async {
        let app = TestApp::start().await;
        let dispatcher = app.token(&app.fx.dispatcher).await;
        let accountant = app.token(&app.fx.accountant).await;
        let customer = app.customer(&dispatcher, "T_PAYER").await;
        let today = chrono::Utc::now().date_naive().to_string();

        let (_, invoice) = app
            .post(
                "/api/v1/finance/invoices",
                &accountant,
                json!({
                    "customer_id": customer,
                    "lines": [{"description": "Customs brokerage", "quantity": "1",
                               "unit_price": "400.00"}]
                }),
            )
            .await;
        let invoice_id = uuid(&invoice["id"]);
        app.post_empty(
            &format!("/api/v1/finance/invoices/{invoice_id}/submit"),
            &accountant,
        )
        .await;
        let (status, issued) = app
            .post(
                &format!("/api/v1/finance/invoices/{invoice_id}/issue"),
                &accountant,
                json!({"issue_date": today}),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{issued}");

        let (status, body) = app
            .post(
                "/api/v1/finance/payments",
                &accountant,
                json!({"invoice_id": invoice_id, "received_on": today,
                       "amount": "500.00", "method": "bank_transfer"}),
            )
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(code(&body), "validation_failed");

        // Paying exactly what is due is fine, which shows the refusal was about the
        // excess and not about payments in general.
        let (status, body) = app
            .post(
                "/api/v1/finance/payments",
                &accountant,
                json!({"invoice_id": invoice_id, "received_on": today,
                       "amount": "400.00", "method": "bank_transfer"}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        assert_eq!(body["status"], "paid");
        assert_eq!(amount(&body["amount_paid"]), 400.0);

        // And a second payment on a settled invoice is refused as well.
        let (status, body) = app
            .post(
                "/api/v1/finance/payments",
                &accountant,
                json!({"invoice_id": invoice_id, "received_on": today,
                       "amount": "1.00", "method": "cash"}),
            )
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(code(&body), "invalid_transition");
    });
}

/// Once a month is closed nothing else lands in it.
#[test]
fn posting_into_a_closed_fiscal_period_is_rejected() {
    run(|| async {
        let app = TestApp::start().await;
        let cfo = app.token(&app.fx.cfo).await;
        let accountant = app.token(&app.fx.accountant).await;
        let today = chrono::Utc::now().date_naive().to_string();
        let period = app.current_period(&cfo).await;

        let entry = json!({
            "entry_date": today,
            "memo": "Accrual",
            "lines": [
                {"account_code": "1000", "debit": "50.00", "credit": "0"},
                {"account_code": "4000", "debit": "0", "credit": "50.00"}
            ]
        });

        // The same entry is accepted while the period is open.
        let (status, body) = app
            .post("/api/v1/finance/journal", &accountant, entry.clone())
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");

        let (status, body) = app
            .post_empty(&format!("/api/v1/finance/periods/{period}/close"), &cfo)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["status"], "closed");

        let (status, body) = app
            .post("/api/v1/finance/journal", &accountant, entry)
            .await;
        assert_eq!(status, StatusCode::CONFLICT, "{body}");
        assert_eq!(code(&body), "conflict");
        assert!(
            body["detail"].as_str().unwrap().contains("closed"),
            "{body}"
        );

        // An accountant cannot simply reopen it either; that needs system:admin.
        let (status, body) = app
            .post_empty(
                &format!("/api/v1/finance/periods/{period}/reopen"),
                &accountant,
            )
            .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
        assert_eq!(code(&body), "forbidden");
    });
}

/// The financial reports are gated on `ledger:read`.
#[test]
fn the_reports_are_closed_to_a_caller_without_ledger_read() {
    run(|| async {
        let app = TestApp::start().await;
        let driver = app.token(&app.fx.driver).await;

        for path in [
            "/api/v1/finance/reports/trial-balance",
            "/api/v1/finance/reports/ar-aging",
            "/api/v1/finance/reports/pnl",
            "/api/v1/finance/accounts",
            "/api/v1/finance/journal",
        ] {
            let (status, body) = app.get(path, &driver).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{path} answered {body}");
            assert_eq!(code(&body), "forbidden", "{path}");
        }

        // The accountant, who does hold ledger:read, gets the report.
        let accountant = app.token(&app.fx.accountant).await;
        let (status, body) = app
            .get("/api/v1/finance/reports/trial-balance", &accountant)
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
    });
}
