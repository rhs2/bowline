//! Fills object storage with the documents the database already claims exist.
//!
//! ```text
//! cargo run --bin backfill-documents                    # render everything that is missing
//! cargo run --bin backfill-documents -- --dry-run       # report what it would do
//! cargo run --bin backfill-documents -- --only invoices # limit the scope
//! ```
//!
//! Two things can be recorded in the database without any bytes behind them: a row in
//! `employee_documents`, and an invoice carrying a `pdf_s3_key`. Seeded demo data is
//! exactly that: rows and keys, no objects, so every download answers `NoSuchKey`.
//! This job walks both sets, checks each key with a HEAD, and asks the billing service
//! to render the ones that are missing. An object that is already there is left alone,
//! so the command is idempotent and safe to re-run.
//!
//! Nothing here invents facts: every value on a rendered page comes from the row it
//! belongs to (or, for a payslip with no payroll item, from the same monthly figure
//! the payroll run itself computes).

use std::collections::HashMap;
use std::path::Path as FsPath;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use bowline_api::clients::billing::BillingClient;
use bowline_api::clients::s3::S3Client;
use bowline_api::finance::invoices;
use bowline_api::{db, telemetry, Config};

const USAGE: &str = "\
bowline backfill-documents: render the employee documents and invoice PDFs that the
database records but object storage does not hold.

Usage:
  backfill-documents [--dry-run] [--only documents|invoices] [--help]

Options:
  --dry-run          report what would be rendered, write nothing
  --only <what>      limit the run to `documents` or `invoices` (default: both)
  --help             print this text

Environment:
  DATABASE_URL, BILLING_URL, INTERNAL_SERVICE_TOKEN, S3_* (see .env.example)";

/// Deductions as a share of gross, matching the rate the seeded payroll run uses, so a
/// payslip rendered without a payroll item still agrees with the ledger.
const DEDUCTION_RATE: Decimal = Decimal::from_parts(28, 0, 0, false, 2);
const MONTHS_PER_YEAR: Decimal = Decimal::from_parts(12, 0, 0, false, 0);
/// Rows between progress lines.
const PROGRESS_EVERY: usize = 100;

// ---------------------------------------------------------------------------
// Options and results
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Both,
    Documents,
    Invoices,
}

impl Scope {
    fn documents(self) -> bool {
        self != Scope::Invoices
    }

    fn invoices(self) -> bool {
        self != Scope::Documents
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub dry_run: bool,
    pub scope: Scope,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            dry_run: false,
            scope: Scope::Both,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Summary {
    pub rendered: usize,
    pub skipped: usize,
    pub failed: usize,
}

impl Summary {
    fn add(&mut self, other: Summary) {
        self.rendered += other.rendered;
        self.skipped += other.skipped;
        self.failed += other.failed;
    }

    fn total(&self) -> usize {
        self.rendered + self.skipped + self.failed
    }

    fn print(&self, dry_run: bool) {
        let verb = if dry_run { "would render" } else { "rendered" };
        println!("\nSummary");
        println!("  {verb:.<20} {:>6}", self.rendered);
        println!("  {:.<20} {:>6}", "already present", self.skipped);
        println!("  {:.<20} {:>6}", "failed", self.failed);
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let options = match parse_args(std::env::args().skip(1))? {
        Some(options) => options,
        None => {
            println!("{USAGE}");
            return Ok(());
        }
    };

    load_dotenv();
    // Like the seeder, this job never signs a token; it shares the service Config only
    // to read DATABASE_URL, BILLING_URL and the S3 settings.
    if !std::env::var("JWT_SECRET").is_ok_and(|v| !v.trim().is_empty()) {
        std::env::set_var(
            "JWT_SECRET",
            "backfill-binary-placeholder-not-used-for-signing",
        );
    }
    let config = Config::from_env().context("loading configuration")?;
    telemetry::init(config.log_format);

    let pool = db::connect(&config.database_url, 4)
        .await
        .context("connecting to postgres")?;
    let result = run(&pool, &config, options).await;
    pool.close().await;

    let summary = result?;
    if summary.failed > 0 {
        bail!(
            "{} document(s) could not be rendered; the errors are above",
            summary.failed
        );
    }
    Ok(())
}

fn parse_args(args: impl Iterator<Item = String>) -> Result<Option<Options>> {
    let mut options = Options::default();
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--help" | "-h" => return Ok(None),
            "--only" => {
                let value = args
                    .next()
                    .context("--only needs a value: documents or invoices")?;
                options.scope = scope(&value)?;
            }
            other if other.starts_with("--only=") => {
                options.scope = scope(other.trim_start_matches("--only="))?;
            }
            other => bail!("unknown argument {other}; try --help"),
        }
    }
    Ok(Some(options))
}

fn scope(value: &str) -> Result<Scope> {
    match value {
        "documents" => Ok(Scope::Documents),
        "invoices" => Ok(Scope::Invoices),
        "both" | "all" => Ok(Scope::Both),
        other => bail!("--only takes documents or invoices, not {other}"),
    }
}

fn load_dotenv() {
    for candidate in [".env", "../.env"] {
        if FsPath::new(candidate).is_file() {
            let _ = dotenvy::from_filename(candidate);
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// The job
// ---------------------------------------------------------------------------

/// Renders every recorded document that object storage does not hold.
///
/// Fails immediately when billing cannot be reached, rather than walking hundreds of
/// rows to report the same connection error on each of them.
pub async fn run(pool: &PgPool, config: &Config, options: Options) -> Result<Summary> {
    let s3 = S3Client::new(&config.s3);
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(3))
        .build()
        .context("building the HTTP client")?;
    let billing = BillingClient::new(http, &config.billing_url, &config.internal_service_token);

    println!("\nBowline document backfill");
    println!("  {:.<20} {}", "billing", config.billing_url);
    println!("  {:.<20} {}", "documents bucket", s3.bucket_documents);
    println!("  {:.<20} {}", "invoice bucket", s3.bucket_pdfs);
    if options.dry_run {
        println!("  {:.<20} dry run, nothing is written", "mode");
    }

    billing.healthz().await.with_context(|| {
        format!(
            "the billing service at {} did not answer its health probe. \
             Start it (./scripts/dev-up.sh, or `make billing`) and run this again",
            config.billing_url
        )
    })?;

    let mut summary = Summary::default();
    if options.scope.documents() {
        summary.add(documents(pool, &s3, &billing, options).await?);
    }
    if options.scope.invoices() {
        summary.add(invoice_pdfs(pool, &s3, &billing, options).await?);
    }
    summary.print(options.dry_run);
    Ok(summary)
}

// ---------------------------------------------------------------------------
// Employee documents
// ---------------------------------------------------------------------------

/// One `employee_documents` row with everything the personnel layouts print.
#[derive(Debug, sqlx::FromRow)]
struct DocumentRow {
    id: Uuid,
    employee_id: Uuid,
    kind: String,
    title: String,
    s3_key: String,
    size_bytes: i64,
    created_at: DateTime<Utc>,
    employee_no: String,
    first_name: String,
    last_name: String,
    email: String,
    hire_date: NaiveDate,
    employment_type: String,
    site: Option<String>,
    pay_grade: Option<String>,
    base_salary: Decimal,
    currency: String,
    position_title: String,
    department_name: String,
    manager_name: Option<String>,
}

impl DocumentRow {
    fn name(&self) -> String {
        format!("{} {}", self.first_name, self.last_name)
    }
}

const DOCUMENT_SELECT: &str = "\
    select d.id, d.employee_id, d.kind, d.title, d.s3_key, d.size_bytes, d.created_at,
           e.employee_no, e.first_name, e.last_name, e.email::text as email, e.hire_date,
           e.employment_type, e.site, e.pay_grade, e.base_salary,
           e.currency::text as currency,
           p.title as position_title, dep.name as department_name,
           m.first_name || ' ' || m.last_name as manager_name
      from employee_documents d
      join employees e on e.id = d.employee_id
      join positions p on p.id = e.position_id
      join departments dep on dep.id = e.department_id
      left join employees m on m.id = e.manager_id
     order by e.employee_no, d.kind, d.created_at";

/// One payroll item, with the period its run belongs to.
#[derive(Debug, Clone, sqlx::FromRow)]
struct PayrollItem {
    employee_id: Uuid,
    year: i16,
    month: i16,
    starts_on: NaiveDate,
    ends_on: NaiveDate,
    gross: Decimal,
    deductions: Decimal,
    net: Decimal,
}

impl PayrollItem {
    fn period(&self) -> String {
        format!("{:04}-{:02}", self.year, self.month)
    }
}

const PAYROLL_SELECT: &str = "\
    select pi.employee_id, fp.year, fp.month, fp.starts_on, fp.ends_on,
           pi.gross, pi.deductions, pi.net
      from payroll_items pi
      join payroll_runs pr on pr.id = pi.run_id
      join fiscal_periods fp on fp.id = pr.period_id";

async fn documents(
    pool: &PgPool,
    s3: &S3Client,
    billing: &BillingClient,
    options: Options,
) -> Result<Summary> {
    let rows: Vec<DocumentRow> = sqlx::query_as(DOCUMENT_SELECT)
        .fetch_all(pool)
        .await
        .context("reading employee_documents")?;
    let items: Vec<PayrollItem> = sqlx::query_as(PAYROLL_SELECT)
        .fetch_all(pool)
        .await
        .context("reading payroll items")?;
    let payroll = Payroll::new(items);

    println!("\nEmployee documents");
    println!("  {} rows to check", rows.len());
    let mut summary = Summary::default();
    for row in &rows {
        let outcome = one_document(pool, s3, billing, &payroll, row, options).await;
        record(&mut summary, &row.s3_key, outcome, options);
        progress(&summary, rows.len());
    }
    section_done(&summary, options);
    Ok(summary)
}

async fn one_document(
    pool: &PgPool,
    s3: &S3Client,
    billing: &BillingClient,
    payroll: &Payroll,
    row: &DocumentRow,
    options: Options,
) -> Result<Outcome> {
    if s3.object_exists(&s3.bucket_documents, &row.s3_key).await? {
        return Ok(Outcome::Present);
    }
    let payload = match document_payload(row, payroll) {
        Some(payload) => payload,
        // `other` is whatever HR chose to upload; there is no layout to regenerate it
        // from, and inventing one would put a document on file that nobody wrote.
        None => return Ok(Outcome::NotRenderable(row.kind.clone())),
    };
    if options.dry_run {
        return Ok(Outcome::Would);
    }
    let rendered = billing.render_document(&payload).await.with_context(|| {
        format!(
            "billing could not render the {} for {}",
            row.kind, row.employee_no
        )
    })?;
    if rendered.s3_key != row.s3_key {
        bail!(
            "billing stored the document as {} but the database expects {}",
            rendered.s3_key,
            row.s3_key
        );
    }
    confirm(s3, &s3.bucket_documents, &row.s3_key).await?;
    // The seeder guessed a size before any bytes existed, so the row advertises one
    // figure while the file is another; the list shows that number, so correct it to
    // what billing actually wrote.
    if !options.dry_run && row.size_bytes != rendered.bytes as i64 {
        sqlx::query("update employee_documents set size_bytes = $1 where id = $2")
            .bind(rendered.bytes as i64)
            .bind(row.id)
            .execute(pool)
            .await
            .with_context(|| format!("recording the real size of {}", row.s3_key))?;
    }
    Ok(Outcome::Rendered(rendered.bytes))
}

/// The `POST /render/document` body for one row. `None` when the kind has no layout.
fn document_payload(row: &DocumentRow, payroll: &Payroll) -> Option<Value> {
    let employee = json!({
        "employee_no": row.employee_no,
        "name": row.name(),
        "email": row.email,
        "position_title": row.position_title,
        "department": row.department_name,
        "site": row.site,
        "pay_grade": row.pay_grade,
        "manager_name": row.manager_name,
        "hire_date": row.hire_date,
        "employment_type": row.employment_type,
    });
    let mut payload = json!({
        "kind": row.kind,
        "s3_key": row.s3_key,
        "title": row.title,
        "employee": employee,
    });
    let details = match row.kind.as_str() {
        "contract" => json!({"contract": {
            "title": row.position_title,
            "department": row.department_name,
            "start_date": row.hire_date,
            "salary": row.base_salary,
            "currency": row.currency,
            "employment_type": row.employment_type,
            "pay_grade": row.pay_grade,
            "site": row.site,
            "weekly_hours": weekly_hours(&row.employment_type),
            "notice_days": notice_days(&row.employment_type),
        }}),
        "payslip" => {
            let pay = payroll.for_document(row);
            json!({"payslip": {
                "period": pay.period,
                "period_start": pay.starts_on,
                "period_end": pay.ends_on,
                "pay_date": pay.ends_on,
                "gross": pay.gross,
                "deductions": pay.deductions,
                "net": pay.net,
                "currency": row.currency,
                "pay_method": "bank_transfer",
            }})
        }
        // The row carries the title and the date HR filed it, and nothing else. The
        // layout leaves out what is not supplied rather than making an issuer up.
        "certificate" => json!({"certificate": {
            "name": row.title,
            "issued_on": row.created_at.date_naive(),
            "reference": row.id,
        }}),
        "id" => json!({"identity": {
            "document_type": row.title,
        }}),
        _ => return None,
    };
    merge(&mut payload, details);
    Some(payload)
}

/// Full time work is a five day week; the shorter contracts are half of it.
fn weekly_hours(employment_type: &str) -> i32 {
    match employment_type {
        "part_time" => 20,
        _ => 40,
    }
}

fn notice_days(employment_type: &str) -> i32 {
    match employment_type {
        "full_time" => 30,
        _ => 14,
    }
}

fn merge(target: &mut Value, extra: Value) {
    if let (Some(target), Some(extra)) = (target.as_object_mut(), extra.as_object()) {
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
}

/// The figures on one payslip.
struct PayPeriod {
    period: String,
    starts_on: NaiveDate,
    ends_on: NaiveDate,
    gross: Decimal,
    deductions: Decimal,
    net: Decimal,
}

/// The payroll items on file, indexed the two ways a payslip needs them.
struct Payroll {
    by_period: HashMap<(Uuid, String), PayrollItem>,
    latest: HashMap<Uuid, PayrollItem>,
}

impl Payroll {
    fn new(items: Vec<PayrollItem>) -> Payroll {
        let mut by_period = HashMap::with_capacity(items.len());
        let mut latest: HashMap<Uuid, PayrollItem> = HashMap::new();
        for item in items {
            latest
                .entry(item.employee_id)
                .and_modify(|current| {
                    if (item.year, item.month) > (current.year, current.month) {
                        *current = item.clone();
                    }
                })
                .or_insert_with(|| item.clone());
            by_period.insert((item.employee_id, item.period()), item);
        }
        Payroll { by_period, latest }
    }

    /// The pay period the document is for, with the run's own figures when payroll has
    /// them and the same monthly calculation payroll uses when it does not.
    fn for_document(&self, row: &DocumentRow) -> PayPeriod {
        let period = period_in(&row.title)
            .or_else(|| period_in(&row.s3_key))
            .or_else(|| self.latest.get(&row.employee_id).map(PayrollItem::period))
            .unwrap_or_else(|| {
                format!("{:04}-{:02}", row.created_at.year(), row.created_at.month())
            });
        if let Some(item) = self.by_period.get(&(row.employee_id, period.clone())) {
            return PayPeriod {
                period,
                starts_on: item.starts_on,
                ends_on: item.ends_on,
                gross: item.gross,
                deductions: item.deductions,
                net: item.net,
            };
        }
        let (starts_on, ends_on) = month_bounds(&period)
            .unwrap_or_else(|| (row.created_at.date_naive(), row.created_at.date_naive()));
        let gross = (row.base_salary / MONTHS_PER_YEAR).round_dp(2);
        let deductions = (gross * DEDUCTION_RATE).round_dp(2);
        PayPeriod {
            period,
            starts_on,
            ends_on,
            gross,
            deductions,
            net: gross - deductions,
        }
    }
}

/// The first `YYYY-MM` in the text, which is how a payslip names its period in both
/// the document title ("Payslip 2026-07") and the object key.
fn period_in(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    for start in 0..bytes.len().saturating_sub(6) {
        let window = &text[start..start + 7];
        let digits =
            |range: std::ops::Range<usize>| window[range].bytes().all(|b| b.is_ascii_digit());
        if digits(0..4) && window.as_bytes()[4] == b'-' && digits(5..7) {
            let month: u32 = window[5..7].parse().ok()?;
            if (1..=12).contains(&month) {
                return Some(window.to_string());
            }
        }
    }
    None
}

fn month_bounds(period: &str) -> Option<(NaiveDate, NaiveDate)> {
    let year: i32 = period.get(0..4)?.parse().ok()?;
    let month: u32 = period.get(5..7)?.parse().ok()?;
    let starts_on = NaiveDate::from_ymd_opt(year, month, 1)?;
    let next = if month == 12 {
        NaiveDate::from_ymd_opt(year + 1, 1, 1)?
    } else {
        NaiveDate::from_ymd_opt(year, month + 1, 1)?
    };
    Some((starts_on, next.pred_opt()?))
}

// ---------------------------------------------------------------------------
// Invoice PDFs
// ---------------------------------------------------------------------------

#[derive(Debug, sqlx::FromRow)]
struct InvoiceTarget {
    id: Uuid,
    invoice_no: String,
    pdf_s3_key: String,
}

async fn invoice_pdfs(
    pool: &PgPool,
    s3: &S3Client,
    billing: &BillingClient,
    options: Options,
) -> Result<Summary> {
    let targets: Vec<InvoiceTarget> = sqlx::query_as(
        "select id, invoice_no, pdf_s3_key from invoices
          where pdf_s3_key is not null order by invoice_no",
    )
    .fetch_all(pool)
    .await
    .context("reading invoices")?;

    println!("\nInvoice PDFs");
    println!("  {} rows to check", targets.len());
    let mut summary = Summary::default();
    for target in &targets {
        let outcome = one_invoice(pool, s3, billing, target, options).await;
        record(&mut summary, &target.pdf_s3_key, outcome, options);
        progress(&summary, targets.len());
    }
    section_done(&summary, options);
    Ok(summary)
}

async fn one_invoice(
    pool: &PgPool,
    s3: &S3Client,
    billing: &BillingClient,
    target: &InvoiceTarget,
    options: Options,
) -> Result<Outcome> {
    if s3
        .object_exists(&s3.bucket_pdfs, &target.pdf_s3_key)
        .await?
    {
        return Ok(Outcome::Present);
    }
    if options.dry_run {
        return Ok(Outcome::Would);
    }
    let mut conn = pool.acquire().await?;
    let detail = invoices::invoice_detail(&mut conn, target.id)
        .await
        .with_context(|| format!("loading invoice {}", target.invoice_no))?;
    drop(conn);
    let rendered = billing
        .render_invoice(&invoices::render_payload(&detail))
        .await
        .with_context(|| format!("billing could not render invoice {}", target.invoice_no))?;
    // Billing derives the invoice key from the invoice number. If the row disagrees,
    // the row is the one that is wrong: the bytes now live where billing put them.
    if rendered.s3_key != target.pdf_s3_key {
        sqlx::query("update invoices set pdf_s3_key = $2 where id = $1")
            .bind(target.id)
            .bind(&rendered.s3_key)
            .execute(pool)
            .await
            .with_context(|| format!("recording the new key for invoice {}", target.invoice_no))?;
        println!(
            "  moved     {} is stored as {}",
            target.pdf_s3_key, rendered.s3_key
        );
    }
    confirm(s3, &s3.bucket_pdfs, &rendered.s3_key).await?;
    Ok(Outcome::Rendered(rendered.bytes))
}

// ---------------------------------------------------------------------------
// Reporting
// ---------------------------------------------------------------------------

enum Outcome {
    /// The object was already in the bucket.
    Present,
    /// Rendered and confirmed, with the size billing reported.
    Rendered(u64),
    /// Missing, and a dry run says so instead of writing.
    Would,
    /// The kind has no layout, so the file cannot be regenerated.
    NotRenderable(String),
}

fn record(summary: &mut Summary, key: &str, outcome: Result<Outcome>, options: Options) {
    match outcome {
        Ok(Outcome::Present) => summary.skipped += 1,
        Ok(Outcome::Rendered(bytes)) => {
            summary.rendered += 1;
            println!("  rendered  {key} ({bytes} bytes)");
        }
        Ok(Outcome::Would) => {
            summary.rendered += 1;
            println!("  missing   {key}");
        }
        Ok(Outcome::NotRenderable(kind)) => {
            summary.failed += 1;
            println!("  FAILED    {key}: no layout for a document of kind {kind}; upload the file instead");
        }
        Err(e) => {
            summary.failed += 1;
            println!("  FAILED    {key}: {e:#}");
        }
    }
    let _ = options;
}

/// A line every hundred rows, so a long run shows it is moving.
fn progress(summary: &Summary, total: usize) {
    let done = summary.total();
    if done.is_multiple_of(PROGRESS_EVERY) && done < total {
        println!(
            "  progress  {done}/{total}  rendered {}  present {}  failed {}",
            summary.rendered, summary.skipped, summary.failed
        );
    }
}

fn section_done(summary: &Summary, options: Options) {
    let verb = if options.dry_run {
        "to render"
    } else {
        "rendered"
    };
    println!(
        "  done      {} rows: {} {verb}, {} already present, {} failed",
        summary.total(),
        summary.rendered,
        summary.skipped,
        summary.failed
    );
}

/// A render is only finished when the object can be seen in the bucket.
async fn confirm(s3: &S3Client, bucket: &str, key: &str) -> Result<()> {
    if s3.object_exists(bucket, key).await? {
        Ok(())
    } else {
        bail!("billing reported success but s3://{bucket}/{key} is still not there")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arguments_select_the_scope() {
        let parse = |args: &[&str]| {
            parse_args(args.iter().map(|s| s.to_string()))
                .unwrap()
                .unwrap()
        };
        assert!(!parse(&[]).dry_run);
        assert_eq!(parse(&[]).scope, Scope::Both);
        assert!(parse(&["--dry-run"]).dry_run);
        assert_eq!(parse(&["--only", "documents"]).scope, Scope::Documents);
        assert_eq!(parse(&["--only=invoices"]).scope, Scope::Invoices);
        assert!(parse_args(["--help".to_string()].into_iter())
            .unwrap()
            .is_none());
        assert!(parse_args(["--only".to_string()].into_iter()).is_err());
        assert!(parse_args(["--only=payslips".to_string()].into_iter()).is_err());
        assert!(parse_args(["--nope".to_string()].into_iter()).is_err());
    }

    #[test]
    fn scopes_select_the_right_halves() {
        assert!(Scope::Both.documents() && Scope::Both.invoices());
        assert!(Scope::Documents.documents() && !Scope::Documents.invoices());
        assert!(!Scope::Invoices.documents() && Scope::Invoices.invoices());
    }

    #[test]
    fn periods_are_found_in_titles_and_keys() {
        assert_eq!(period_in("Payslip 2026-07").as_deref(), Some("2026-07"));
        assert_eq!(
            period_in("employees/8f1/payslip-2025-12.pdf").as_deref(),
            Some("2025-12")
        );
        assert_eq!(period_in("2026-13").as_deref(), None);
        assert_eq!(period_in("Employment contract").as_deref(), None);
        assert_eq!(period_in("").as_deref(), None);
    }

    #[test]
    fn month_bounds_cover_the_whole_month() {
        assert_eq!(
            month_bounds("2026-07"),
            Some((
                NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 7, 31).unwrap()
            ))
        );
        assert_eq!(
            month_bounds("2024-02"),
            Some((
                NaiveDate::from_ymd_opt(2024, 2, 1).unwrap(),
                NaiveDate::from_ymd_opt(2024, 2, 29).unwrap()
            ))
        );
        assert_eq!(
            month_bounds("2026-12"),
            Some((
                NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()
            ))
        );
        assert_eq!(month_bounds("later"), None);
    }

    fn row(kind: &str, title: &str, key: &str) -> DocumentRow {
        DocumentRow {
            id: Uuid::nil(),
            employee_id: Uuid::nil(),
            kind: kind.to_string(),
            title: title.to_string(),
            s3_key: key.to_string(),
            // deliberately wrong, the way a seeded row is, so a test that checks the
            // correction has something to correct
            size_bytes: 123_456,
            created_at: "2026-08-01T09:00:00Z".parse().unwrap(),
            employee_no: "BWL-000482".to_string(),
            first_name: "Priya".to_string(),
            last_name: "Raman".to_string(),
            email: "priya.raman@bowline.example".to_string(),
            hire_date: NaiveDate::from_ymd_opt(2022, 3, 14).unwrap(),
            employment_type: "full_time".to_string(),
            site: Some("Port City Terminal".to_string()),
            pay_grade: Some("G5".to_string()),
            base_salary: Decimal::new(6840000, 2),
            currency: "USD".to_string(),
            position_title: "Warehouse Supervisor".to_string(),
            department_name: "Warehouse Operations".to_string(),
            manager_name: Some("Marcus Elliot".to_string()),
        }
    }

    #[test]
    fn contract_payload_carries_the_employment_terms() {
        let row = row(
            "contract",
            "Employment contract",
            "employees/x/contract.pdf",
        );
        let payload =
            document_payload(&row, &Payroll::new(Vec::new())).expect("a contract renders");
        assert_eq!(payload["kind"], "contract");
        assert_eq!(payload["s3_key"], "employees/x/contract.pdf");
        assert_eq!(payload["employee"]["name"], "Priya Raman");
        assert_eq!(payload["contract"]["title"], "Warehouse Supervisor");
        assert_eq!(payload["contract"]["start_date"], "2022-03-14");
        assert_eq!(payload["contract"]["weekly_hours"], 40);
        assert_eq!(payload["contract"]["notice_days"], 30);
    }

    #[test]
    fn payslip_without_a_payroll_item_uses_the_monthly_figure() {
        let row = row(
            "payslip",
            "Payslip 2026-07",
            "employees/x/payslip-2026-07.pdf",
        );
        let payload = document_payload(&row, &Payroll::new(Vec::new())).expect("a payslip renders");
        let payslip = &payload["payslip"];
        assert_eq!(payslip["period"], "2026-07");
        assert_eq!(payslip["period_start"], "2026-07-01");
        assert_eq!(payslip["period_end"], "2026-07-31");
        // 68,400.00 a year is 5,700.00 a month, less 28 per cent.
        assert_eq!(payslip["gross"], "5700.00");
        assert_eq!(payslip["deductions"], "1596.00");
        assert_eq!(payslip["net"], "4104.00");
    }

    #[test]
    fn payslip_prefers_the_payroll_item_for_that_month() {
        let row = row(
            "payslip",
            "Payslip 2026-07",
            "employees/x/payslip-2026-07.pdf",
        );
        let payroll = Payroll::new(vec![PayrollItem {
            employee_id: row.employee_id,
            year: 2026,
            month: 7,
            starts_on: NaiveDate::from_ymd_opt(2026, 7, 1).unwrap(),
            ends_on: NaiveDate::from_ymd_opt(2026, 7, 31).unwrap(),
            gross: Decimal::new(600000, 2),
            deductions: Decimal::new(150000, 2),
            net: Decimal::new(450000, 2),
        }]);
        let payload = document_payload(&row, &payroll).expect("a payslip renders");
        assert_eq!(payload["payslip"]["gross"], "6000.00");
        assert_eq!(payload["payslip"]["net"], "4500.00");
    }

    #[test]
    fn certificate_and_identity_only_state_what_is_on_file() {
        let payroll = Payroll::new(Vec::new());
        let certificate = document_payload(
            &row(
                "certificate",
                "Forklift certificate",
                "employees/x/certificate.pdf",
            ),
            &payroll,
        )
        .expect("a certificate renders");
        assert_eq!(certificate["certificate"]["name"], "Forklift certificate");
        assert_eq!(certificate["certificate"]["issued_on"], "2026-08-01");
        assert!(certificate["certificate"].get("issuer").is_none());

        let identity = document_payload(
            &row("id", "Identity document", "employees/x/id.pdf"),
            &payroll,
        )
        .expect("an identity record renders");
        assert_eq!(identity["identity"]["document_type"], "Identity document");
        assert!(identity["identity"].get("number").is_none());
    }

    #[test]
    fn an_uploaded_file_has_no_layout_to_regenerate() {
        let row = row(
            "other",
            "Signed parking permit",
            "employees/x/other/permit.pdf",
        );
        assert!(document_payload(&row, &Payroll::new(Vec::new())).is_none());
    }
}
