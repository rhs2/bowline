//! List envelope and paging parameters shared by every list endpoint.

use serde::{Deserialize, Serialize};
use sqlx::postgres::PgRow;
use sqlx::{FromRow, Row};
use utoipa::{IntoParams, ToSchema};

pub const MAX_PER_PAGE: u32 = 100;

#[derive(Debug, Clone, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// 1-based page number
    pub page: Option<u32>,
    /// rows per page, at most 100
    pub per_page: Option<u32>,
    pub sort: Option<String>,
    /// `asc` or `desc`
    pub order: Option<String>,
    /// free-text search
    pub q: Option<String>,
}

#[derive(Debug, Clone, Copy)]
pub struct Page {
    pub page: u32,
    pub per_page: u32,
}

impl Page {
    pub fn limit(&self) -> i64 {
        self.per_page as i64
    }

    pub fn offset(&self) -> i64 {
        (self.page as i64 - 1) * self.per_page as i64
    }
}

impl PageQuery {
    pub fn page(&self) -> Page {
        Page {
            page: self.page.unwrap_or(1).max(1),
            per_page: self.per_page.unwrap_or(25).clamp(1, MAX_PER_PAGE),
        }
    }

    /// Picks the sort column from an allow list; the first entry is the default.
    pub fn order_by(&self, allowed: &[(&str, &str)]) -> String {
        let (_, default_col) = allowed[0];
        let col = self
            .sort
            .as_deref()
            .and_then(|s| allowed.iter().find(|(name, _)| *name == s))
            .map(|(_, col)| *col)
            .unwrap_or(default_col);
        let dir = match self.order.as_deref() {
            Some("desc") => "desc",
            Some("asc") => "asc",
            _ => {
                if self.sort.is_none() {
                    "desc"
                } else {
                    "asc"
                }
            }
        };
        format!("{col} {dir}")
    }

    pub fn search(&self) -> Option<String> {
        self.q
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{s}%"))
    }
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PageOut<T> {
    pub items: Vec<T>,
    pub page: u32,
    pub per_page: u32,
    pub total: i64,
}

impl<T> PageOut<T> {
    pub fn new(items: Vec<T>, page: Page, total: i64) -> Self {
        PageOut {
            items,
            page: page.page,
            per_page: page.per_page,
            total,
        }
    }
}

/// Splits rows that carry a `total_count` window column into items and the total.
pub fn split_total<T>(rows: Vec<PgRow>) -> Result<(Vec<T>, i64), sqlx::Error>
where
    T: for<'r> FromRow<'r, PgRow>,
{
    let total = rows
        .first()
        .map(|r| r.try_get::<i64, _>("total_count"))
        .transpose()?
        .unwrap_or(0);
    let items = rows
        .iter()
        .map(T::from_row)
        .collect::<Result<Vec<_>, _>>()?;
    Ok((items, total))
}
