//! Hierarchy scope: which employees (and rows attached to employees) a principal
//! may see for a given permission family.

use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use crate::auth::principal::Principal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Own,
    Subtree,
    Department,
    All,
}

/// Concrete values behind a scope, ready to be bound into a query.
#[derive(Debug, Clone)]
pub struct ScopeFilter {
    pub scope: Scope,
    pub me: Uuid,
    pub path: String,
    pub department_ids: Vec<Uuid>,
}

impl ScopeFilter {
    pub fn new(principal: &Principal, scope: Scope) -> Self {
        ScopeFilter {
            scope,
            me: principal.employee_id,
            path: principal.path.clone(),
            department_ids: principal.department_ids.clone(),
        }
    }

    /// Appends the predicate for an `employees` table aliased `alias`.
    pub fn push(&self, qb: &mut QueryBuilder<'_, Postgres>, alias: &str) {
        match self.scope {
            Scope::All => {
                qb.push(" true ");
            }
            Scope::Own => {
                qb.push(format!(" {alias}.id = "));
                qb.push_bind(self.me);
            }
            Scope::Subtree => {
                qb.push(format!(" {alias}.path <@ "));
                qb.push_bind(self.path.clone());
                qb.push("::ltree ");
            }
            Scope::Department => {
                qb.push(format!(" {alias}.department_id = any("));
                qb.push_bind(self.department_ids.clone());
                qb.push(") ");
            }
        }
    }

    /// In-memory version of the same predicate for a single employee.
    pub fn contains(&self, employee_id: Uuid, path: &str, department_id: Uuid) -> bool {
        match self.scope {
            Scope::All => true,
            Scope::Own => employee_id == self.me,
            Scope::Subtree => path == self.path || path.starts_with(&format!("{}.", self.path)),
            Scope::Department => self.department_ids.contains(&department_id),
        }
    }
}
