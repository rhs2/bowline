//! The one way money reaches the ledger.
//!
//! Every source (invoice, payment, expense, payroll, vendor bill, manual entry and
//! reversal) builds a [`Posting`] and hands it to [`post`], which writes the
//! `journal_entries` row and all of its `journal_lines` inside the caller's
//! transaction. The database owns the integrity rules:
//!
//! * `journal_lines_balanced` is a deferred constraint trigger, so the entry and its
//!   lines have to be written in one transaction. After the lines are in, this module
//!   flips the constraint to `immediate` for one round trip so an unbalanced entry is
//!   reported where the caller can turn it into a clean 422 instead of blowing up at
//!   commit time, then puts it back to `deferred` for the rest of the transaction.
//! * The `journal_entries` guard refuses anything aimed at a closed period; the period
//!   is looked up here first so the usual answer is a readable 409.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::json;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::audit::{self, AuditCtx};
use crate::error::{ApiError, ApiResult, FieldError};

/// Chart of accounts codes the API posts to by name.
pub const CASH: &str = "1000";
pub const ACCOUNTS_RECEIVABLE: &str = "1100";
pub const ACCOUNTS_PAYABLE: &str = "2000";
pub const SALARIES_PAYABLE: &str = "2100";
pub const TAXES_PAYABLE: &str = "2200";
pub const FREIGHT_REVENUE: &str = "4000";
pub const SALARIES: &str = "5100";
pub const FUEL: &str = "5200";
pub const OFFICE_AND_ADMIN: &str = "5400";
pub const TRAVEL_AND_MEALS: &str = "5700";

/// Expense account used for each expense claim category.
pub fn expense_account_for(category: &str) -> &'static str {
    match category {
        "fuel" => FUEL,
        "travel" | "meals" => TRAVEL_AND_MEALS,
        _ => OFFICE_AND_ADMIN,
    }
}

#[derive(Debug, Clone)]
pub struct PostingLine {
    pub account_code: String,
    pub debit: Decimal,
    pub credit: Decimal,
    pub description: Option<String>,
}

impl PostingLine {
    pub fn debit(
        account_code: &str,
        amount: Decimal,
        description: impl Into<String>,
    ) -> PostingLine {
        PostingLine {
            account_code: account_code.to_string(),
            debit: amount.round_dp(2),
            credit: Decimal::ZERO,
            description: Some(description.into()),
        }
    }

    pub fn credit(
        account_code: &str,
        amount: Decimal,
        description: impl Into<String>,
    ) -> PostingLine {
        PostingLine {
            account_code: account_code.to_string(),
            debit: Decimal::ZERO,
            credit: amount.round_dp(2),
            description: Some(description.into()),
        }
    }
}

/// One journal entry, ready to be written.
#[derive(Debug, Clone)]
pub struct Posting {
    pub entry_date: NaiveDate,
    pub memo: String,
    /// One of the `journal_entries.source_type` values.
    pub source_type: &'static str,
    pub source_id: Option<Uuid>,
    pub reverses_entry_id: Option<Uuid>,
    pub lines: Vec<PostingLine>,
}

impl Posting {
    pub fn new(
        entry_date: NaiveDate,
        memo: impl Into<String>,
        source_type: &'static str,
        source_id: Option<Uuid>,
    ) -> Posting {
        Posting {
            entry_date,
            memo: memo.into(),
            source_type,
            source_id,
            reverses_entry_id: None,
            lines: Vec::new(),
        }
    }

    pub fn with_lines(mut self, lines: Vec<PostingLine>) -> Posting {
        self.lines = lines;
        self
    }

    pub fn total_debit(&self) -> Decimal {
        self.lines.iter().map(|l| l.debit).sum()
    }

    pub fn total_credit(&self) -> Decimal {
        self.lines.iter().map(|l| l.credit).sum()
    }
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PostedEntry {
    pub id: Uuid,
    pub entry_no: i64,
    pub period_id: Uuid,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PeriodRef {
    pub id: Uuid,
    pub year: i16,
    pub month: i16,
    pub starts_on: NaiveDate,
    pub ends_on: NaiveDate,
    pub status: String,
}

/// The open fiscal period covering `date`, or a problem the caller can return as is.
pub async fn open_period_for(conn: &mut PgConnection, date: NaiveDate) -> ApiResult<PeriodRef> {
    let period: Option<PeriodRef> = sqlx::query_as(
        "select id, year, month, starts_on, ends_on, status from fiscal_periods
          where $1 between starts_on and ends_on",
    )
    .bind(date)
    .fetch_optional(&mut *conn)
    .await?;
    let period = period
        .ok_or_else(|| ApiError::validation("entry_date", "no fiscal period covers that date"))?;
    if period.status != "open" {
        return Err(ApiError::conflict(format!(
            "fiscal period {}-{:02} is closed",
            period.year, period.month
        )));
    }
    Ok(period)
}

pub async fn account_id(conn: &mut PgConnection, code: &str) -> ApiResult<Uuid> {
    sqlx::query_scalar("select id from accounts where code = $1 and active")
        .bind(code)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::internal_msg(format!("chart of accounts is missing code {code}")))
}

fn validate(posting: &Posting) -> ApiResult<()> {
    let mut errors: Vec<FieldError> = Vec::new();
    if posting.lines.len() < 2 {
        errors.push(FieldError::new(
            "lines",
            "an entry needs at least two lines",
        ));
    }
    for (idx, line) in posting.lines.iter().enumerate() {
        let field = format!("lines[{idx}]");
        if line.debit < Decimal::ZERO || line.credit < Decimal::ZERO {
            errors.push(FieldError::new(&field, "amounts must not be negative"));
        } else if line.debit > Decimal::ZERO && line.credit > Decimal::ZERO {
            errors.push(FieldError::new(
                &field,
                "a line is either a debit or a credit",
            ));
        } else if line.debit == Decimal::ZERO && line.credit == Decimal::ZERO {
            errors.push(FieldError::new(
                &field,
                "a line needs a debit or a credit above zero",
            ));
        }
    }
    if posting.total_debit() != posting.total_credit() {
        errors.push(FieldError::new(
            "lines",
            format!(
                "the entry does not balance: debits {} against credits {}",
                posting.total_debit(),
                posting.total_credit()
            ),
        ));
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ApiError::Validation(errors))
    }
}

async fn resolve_accounts(conn: &mut PgConnection, posting: &Posting) -> ApiResult<Vec<Uuid>> {
    let codes: Vec<String> = posting
        .lines
        .iter()
        .map(|l| l.account_code.trim().to_string())
        .collect();
    let known: Vec<(String, Uuid)> =
        sqlx::query_as("select code, id from accounts where code = any($1) and active")
            .bind(&codes)
            .fetch_all(conn)
            .await?;
    let mut ids = Vec::with_capacity(codes.len());
    let mut errors: Vec<FieldError> = Vec::new();
    for (idx, code) in codes.iter().enumerate() {
        match known.iter().find(|(c, _)| c == code) {
            Some((_, id)) => ids.push(*id),
            None => errors.push(FieldError::new(
                format!("lines[{idx}].account_code"),
                format!("unknown or inactive account {code}"),
            )),
        }
    }
    if errors.is_empty() {
        Ok(ids)
    } else {
        Err(ApiError::Validation(errors))
    }
}

/// Runs the deferred balance trigger now so an unbalanced entry becomes a 422 here
/// rather than an opaque failure at commit, then restores the deferred behaviour.
async fn check_balanced(conn: &mut PgConnection) -> ApiResult<()> {
    if let Err(err) = sqlx::query("set constraints journal_lines_balanced immediate")
        .execute(&mut *conn)
        .await
    {
        return Err(balance_problem(err));
    }
    sqlx::query("set constraints journal_lines_balanced deferred")
        .execute(conn)
        .await?;
    Ok(())
}

fn balance_problem(err: sqlx::Error) -> ApiError {
    if let sqlx::Error::Database(db) = &err {
        if db.message().contains("not balanced") {
            return ApiError::Validation(vec![FieldError::new(
                "lines",
                "the entry does not balance: total debits must equal total credits",
            )]);
        }
    }
    ApiError::from(err)
}

/// Writes one journal entry and its lines inside the caller's transaction.
pub async fn post(
    conn: &mut PgConnection,
    ctx: &AuditCtx,
    posted_by: Uuid,
    posting: Posting,
) -> ApiResult<PostedEntry> {
    validate(&posting)?;
    let period = open_period_for(&mut *conn, posting.entry_date).await?;
    let account_ids = resolve_accounts(&mut *conn, &posting).await?;
    let entry: PostedEntry = sqlx::query_as(
        "insert into journal_entries (period_id, entry_date, memo, source_type, source_id,
                                      posted_by, reverses_entry_id)
         values ($1, $2, $3, $4, $5, $6, $7) returning id, entry_no, period_id",
    )
    .bind(period.id)
    .bind(posting.entry_date)
    .bind(&posting.memo)
    .bind(posting.source_type)
    .bind(posting.source_id)
    .bind(posted_by)
    .bind(posting.reverses_entry_id)
    .fetch_one(&mut *conn)
    .await?;
    let debits: Vec<Decimal> = posting.lines.iter().map(|l| l.debit).collect();
    let credits: Vec<Decimal> = posting.lines.iter().map(|l| l.credit).collect();
    let descriptions: Vec<Option<String>> = posting
        .lines
        .iter()
        .map(|l| l.description.clone())
        .collect();
    // One statement, so the row trigger sees the whole entry when it fires.
    sqlx::query(
        "insert into journal_lines (entry_id, account_id, debit, credit, description)
         select $1, l.account_id, l.debit, l.credit, l.description
           from unnest($2::uuid[], $3::numeric[], $4::numeric[], $5::text[])
             as l(account_id, debit, credit, description)",
    )
    .bind(entry.id)
    .bind(&account_ids)
    .bind(&debits)
    .bind(&credits)
    .bind(&descriptions)
    .execute(&mut *conn)
    .await?;
    check_balanced(&mut *conn).await?;
    let after = json!({
        "entry_no": entry.entry_no,
        "period_id": period.id,
        "entry_date": posting.entry_date,
        "memo": posting.memo,
        "source_type": posting.source_type,
        "source_id": posting.source_id,
        "reverses_entry_id": posting.reverses_entry_id,
        "lines": posting.lines.iter().map(|l| json!({
            "account_code": l.account_code,
            "debit": l.debit,
            "credit": l.credit,
            "description": l.description,
        })).collect::<Vec<_>>(),
    });
    audit::record(
        conn,
        ctx,
        "ledger.post",
        "journal_entry",
        Some(entry.id),
        None,
        Some(after),
    )
    .await?;
    Ok(entry)
}

#[derive(sqlx::FromRow)]
struct ReversalSource {
    memo: String,
    source_id: Option<Uuid>,
    reversed_by_entry_id: Option<Uuid>,
}

/// Posts the mirror of an entry and links the two together in both directions.
/// The reversal lands in the period covering `entry_date`, which lets an entry in a
/// closed month be reversed in the open one.
pub async fn reverse(
    conn: &mut PgConnection,
    ctx: &AuditCtx,
    posted_by: Uuid,
    entry_id: Uuid,
    entry_date: NaiveDate,
    memo: Option<String>,
) -> ApiResult<PostedEntry> {
    let original: ReversalSource = sqlx::query_as(
        "select memo, source_id, reversed_by_entry_id from journal_entries
          where id = $1 for update",
    )
    .bind(entry_id)
    .fetch_optional(&mut *conn)
    .await?
    .ok_or_else(|| ApiError::not_found("journal entry"))?;
    if let Some(existing) = original.reversed_by_entry_id {
        return Err(ApiError::conflict(format!(
            "entry has already been reversed by {existing}"
        )));
    }
    let lines: Vec<(String, Decimal, Decimal, Option<String>)> = sqlx::query_as(
        "select a.code, l.debit, l.credit, l.description
           from journal_lines l join accounts a on a.id = l.account_id
          where l.entry_id = $1 order by l.id",
    )
    .bind(entry_id)
    .fetch_all(&mut *conn)
    .await?;
    let mirrored: Vec<PostingLine> = lines
        .into_iter()
        .map(|(code, debit, credit, description)| PostingLine {
            account_code: code,
            debit: credit,
            credit: debit,
            description,
        })
        .collect();
    let memo = memo.unwrap_or_else(|| format!("Reversal of: {}", original.memo));
    let mut posting = Posting::new(entry_date, memo, "reversal", original.source_id);
    posting.reverses_entry_id = Some(entry_id);
    posting.lines = mirrored;
    let reversal = post(&mut *conn, ctx, posted_by, posting).await?;
    sqlx::query("update journal_entries set reversed_by_entry_id = $2 where id = $1")
        .bind(entry_id)
        .bind(reversal.id)
        .execute(&mut *conn)
        .await?;
    audit::record(
        conn,
        ctx,
        "journal.reverse",
        "journal_entry",
        Some(entry_id),
        Some(json!({"reversed_by_entry_id": null})),
        Some(json!({"reversed_by_entry_id": reversal.id, "entry_no": reversal.entry_no})),
    )
    .await?;
    Ok(reversal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(code: &str, debit: i64, credit: i64) -> PostingLine {
        PostingLine {
            account_code: code.to_string(),
            debit: Decimal::from(debit),
            credit: Decimal::from(credit),
            description: None,
        }
    }

    fn posting(lines: Vec<PostingLine>) -> Posting {
        Posting::new(
            NaiveDate::from_ymd_opt(2026, 3, 1).unwrap(),
            "test",
            "manual",
            None,
        )
        .with_lines(lines)
    }

    #[test]
    fn balanced_entries_pass() {
        let p = posting(vec![line(CASH, 100, 0), line(FREIGHT_REVENUE, 0, 100)]);
        assert!(validate(&p).is_ok());
    }

    #[test]
    fn unbalanced_entries_are_rejected() {
        let p = posting(vec![line(CASH, 100, 0), line(FREIGHT_REVENUE, 0, 90)]);
        let err = validate(&p).unwrap_err();
        assert_eq!(err.code(), "validation_failed");
    }

    #[test]
    fn a_line_is_a_debit_or_a_credit() {
        let p = posting(vec![line(CASH, 100, 100), line(FREIGHT_REVENUE, 0, 0)]);
        assert!(validate(&p).is_err());
    }

    #[test]
    fn one_sided_entries_are_rejected() {
        let p = posting(vec![line(CASH, 0, 0)]);
        assert!(validate(&p).is_err());
    }

    #[test]
    fn expense_categories_map_to_accounts() {
        assert_eq!(expense_account_for("fuel"), FUEL);
        assert_eq!(expense_account_for("travel"), TRAVEL_AND_MEALS);
        assert_eq!(expense_account_for("meals"), TRAVEL_AND_MEALS);
        assert_eq!(expense_account_for("supplies"), OFFICE_AND_ADMIN);
        assert_eq!(expense_account_for("other"), OFFICE_AND_ADMIN);
    }
}
