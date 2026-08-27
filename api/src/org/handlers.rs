//! Organisation endpoints: tree, departments, positions, employees.

use std::collections::HashMap;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::{PgConnection, Postgres, QueryBuilder};
use utoipa::{IntoParams, OpenApi, ToSchema};
use uuid::Uuid;
use validator::Validate;

use crate::audit;
use crate::auth::Actor;
use crate::error::{ApiError, ApiResult, Problem};
use crate::http::extract::ValidatedJson;
use crate::http::pagination::{split_total, PageOut, PageQuery};
use crate::org::service::{self, ChainEntry};
use crate::scope::Scope;
use crate::state::AppState;

#[derive(Debug, Serialize, ToSchema)]
pub struct TreeNode {
    pub id: Uuid,
    pub name: String,
    pub title: String,
    pub level: i16,
    pub department: String,
    /// The tree is self-referencing, so the schema must reference this type by name
    /// rather than inline it; without this the OpenAPI document recurses forever.
    #[schema(no_recursion)]
    pub children: Vec<TreeNode>,
}

#[derive(sqlx::FromRow)]
struct TreeRow {
    id: Uuid,
    name: String,
    title: String,
    level: i16,
    department: String,
    manager_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/org/tree", tag = "org", security(("bearer" = [])),
    responses((status = 200, body = TreeNode)))]
pub async fn tree(State(state): State<AppState>, actor: Actor) -> ApiResult<Json<TreeNode>> {
    actor.require("org:read")?;
    let rows: Vec<TreeRow> = sqlx::query_as(
        "select e.id, e.first_name || ' ' || e.last_name as name, p.title, p.level, d.name as department, e.manager_id
           from employees e join positions p on p.id = e.position_id join departments d on d.id = e.department_id
          where e.status <> 'terminated'
          order by p.level, e.last_name, e.first_name",
    )
    .fetch_all(&state.pool)
    .await?;
    let mut children: HashMap<Option<Uuid>, Vec<&TreeRow>> = HashMap::new();
    for row in &rows {
        children.entry(row.manager_id).or_default().push(row);
    }
    fn build(row: &TreeRow, children: &HashMap<Option<Uuid>, Vec<&TreeRow>>) -> TreeNode {
        TreeNode {
            id: row.id,
            name: row.name.clone(),
            title: row.title.clone(),
            level: row.level,
            department: row.department.clone(),
            children: children
                .get(&Some(row.id))
                .map(|kids| kids.iter().map(|k| build(k, children)).collect())
                .unwrap_or_default(),
        }
    }
    let root = children
        .get(&None)
        .and_then(|roots| roots.first().copied())
        .ok_or_else(|| ApiError::not_found("organisation root"))?;
    Ok(Json(build(root, &children)))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DepartmentHead {
    pub id: Uuid,
    pub name: String,
    pub title: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct DepartmentOut {
    pub id: Uuid,
    pub code: String,
    pub name: String,
    pub parent_id: Option<Uuid>,
    pub cost_center: Option<String>,
    pub headcount: i64,
    pub head: Option<DepartmentHead>,
}

#[derive(sqlx::FromRow)]
struct DepartmentRow {
    id: Uuid,
    code: String,
    name: String,
    parent_id: Option<Uuid>,
    cost_center: Option<String>,
    headcount: i64,
    head_id: Option<Uuid>,
    head_name: Option<String>,
    head_title: Option<String>,
}

#[utoipa::path(get, path = "/api/v1/org/departments", tag = "org", security(("bearer" = [])),
    responses((status = 200, body = Vec<DepartmentOut>)))]
pub async fn departments(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Vec<DepartmentOut>>> {
    actor.require("org:read")?;
    let rows: Vec<DepartmentRow> = sqlx::query_as(
        "select d.id, d.code::text as code, d.name, d.parent_id, d.cost_center,
                (select count(*) from employees e where e.department_id = d.id and e.status <> 'terminated') as headcount,
                h.id as head_id, h.first_name || ' ' || h.last_name as head_name, h.title as head_title
           from departments d
           left join lateral (
                select e.id, e.first_name, e.last_name, p.title
                  from employees e join positions p on p.id = e.position_id
                 where e.department_id = d.id and e.status <> 'terminated'
                 order by p.level, e.hire_date limit 1) h on true
          order by d.parent_id nulls first, d.name",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(
        rows.into_iter()
            .map(|r| DepartmentOut {
                id: r.id,
                code: r.code,
                name: r.name,
                parent_id: r.parent_id,
                cost_center: r.cost_center,
                headcount: r.headcount,
                head: match (r.head_id, r.head_name, r.head_title) {
                    (Some(id), Some(name), Some(title)) => Some(DepartmentHead { id, name, title }),
                    _ => None,
                },
            })
            .collect(),
    ))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct PositionOut {
    pub id: Uuid,
    pub code: String,
    pub title: String,
    pub level: i16,
    pub department_id: Option<Uuid>,
    pub is_people_manager: bool,
}

#[utoipa::path(get, path = "/api/v1/org/positions", tag = "org", security(("bearer" = [])),
    responses((status = 200, body = Vec<PositionOut>)))]
pub async fn positions(
    State(state): State<AppState>,
    actor: Actor,
) -> ApiResult<Json<Vec<PositionOut>>> {
    actor.require("org:read")?;
    let rows: Vec<PositionOut> = sqlx::query_as(
        "select id, code::text as code, title, level, department_id, is_people_manager from positions order by level, title",
    )
    .fetch_all(&state.pool)
    .await?;
    Ok(Json(rows))
}

#[derive(Debug, Serialize, ToSchema, sqlx::FromRow)]
pub struct EmployeeSummary {
    pub id: Uuid,
    pub employee_no: String,
    pub first_name: String,
    pub last_name: String,
    pub email: String,
    pub phone: Option<String>,
    pub title: String,
    pub level: i16,
    pub position_id: Uuid,
    pub department_id: Uuid,
    pub department_name: String,
    pub manager_id: Option<Uuid>,
    pub status: String,
    pub employment_type: String,
    pub site: Option<String>,
    pub hire_date: NaiveDate,
}

const EMPLOYEE_SELECT: &str =
    "select e.id, e.employee_no, e.first_name, e.last_name, e.email::text as email, e.phone, p.title, p.level,
            e.position_id, e.department_id, d.name as department_name, e.manager_id, e.status, e.employment_type,
            e.site, e.hire_date, count(*) over() as total_count
       from employees e join positions p on p.id = e.position_id join departments d on d.id = e.department_id
      where ";

#[derive(Debug, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EmployeeFilter {
    pub department_id: Option<Uuid>,
    pub status: Option<String>,
    pub level: Option<i16>,
    pub manager_id: Option<Uuid>,
}

#[utoipa::path(get, path = "/api/v1/employees", tag = "org", security(("bearer" = [])),
    params(PageQuery, EmployeeFilter), responses((status = 200, body = PageOut<EmployeeSummary>)))]
pub async fn list_employees(
    State(state): State<AppState>,
    actor: Actor,
    Query(page): Query<PageQuery>,
    Query(filter): Query<EmployeeFilter>,
) -> ApiResult<Json<PageOut<EmployeeSummary>>> {
    let scope = actor.scope_filter("employees:read")?;
    let mut qb: QueryBuilder<Postgres> = QueryBuilder::new(EMPLOYEE_SELECT);
    scope.push(&mut qb, "e");
    if let Some(dept) = filter.department_id {
        qb.push(" and e.department_id = ").push_bind(dept);
    }
    if let Some(status) = &filter.status {
        qb.push(" and e.status = ").push_bind(status.clone());
    } else {
        qb.push(" and e.status <> 'terminated'");
    }
    if let Some(level) = filter.level {
        qb.push(" and p.level = ").push_bind(level);
    }
    if let Some(manager) = filter.manager_id {
        qb.push(" and e.manager_id = ").push_bind(manager);
    }
    if let Some(q) = page.search() {
        qb.push(" and (e.first_name || ' ' || e.last_name ilike ")
            .push_bind(q.clone())
            .push(" or e.email::text ilike ")
            .push_bind(q.clone())
            .push(" or e.employee_no ilike ")
            .push_bind(q)
            .push(" or p.title ilike ")
            .push_bind(page.search().unwrap_or_default())
            .push(")");
    }
    let order = page.order_by(&[
        ("name", "e.last_name asc, e.first_name"),
        ("level", "p.level"),
        ("hire_date", "e.hire_date"),
        ("employee_no", "e.employee_no"),
        ("department", "d.name"),
    ]);
    let order = if page.sort.is_none() {
        "e.last_name asc, e.first_name asc".to_string()
    } else {
        order
    };
    qb.push(format!(" order by {order} limit "));
    let paging = page.page();
    qb.push_bind(paging.limit())
        .push(" offset ")
        .push_bind(paging.offset());
    let rows = qb.build().fetch_all(&state.pool).await?;
    let (items, total) = split_total::<EmployeeSummary>(rows)?;
    Ok(Json(PageOut::new(items, paging, total)))
}

#[derive(Debug, Serialize, ToSchema)]
pub struct ManagerRef {
    pub id: Uuid,
    pub name: String,
    pub title: String,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct EmployeeDetail {
    #[serde(flatten)]
    pub summary: EmployeeSummary,
    pub pay_grade: Option<String>,
    pub base_salary: Option<Decimal>,
    pub currency: String,
    pub termination_date: Option<NaiveDate>,
    pub manager: Option<ManagerRef>,
    pub direct_reports_count: i64,
    pub user_id: Option<Uuid>,
    pub user_status: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(sqlx::FromRow)]
struct DetailRow {
    pay_grade: Option<String>,
    base_salary: Decimal,
    currency: String,
    termination_date: Option<NaiveDate>,
    manager_name: Option<String>,
    manager_title: Option<String>,
    direct_reports_count: i64,
    user_id: Option<Uuid>,
    user_status: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

async fn employee_detail(
    conn: &mut PgConnection,
    actor: &Actor,
    id: Uuid,
) -> ApiResult<EmployeeDetail> {
    let summary: EmployeeSummary = sqlx::query_as(&format!("{EMPLOYEE_SELECT} e.id = $1"))
        .bind(id)
        .fetch_one(&mut *conn)
        .await?;
    let extra: DetailRow = sqlx::query_as(
        "select e.pay_grade, e.base_salary, e.currency, e.termination_date,
                m.first_name || ' ' || m.last_name as manager_name, mp.title as manager_title,
                (select count(*) from employees r where r.manager_id = e.id and r.status <> 'terminated') as direct_reports_count,
                u.id as user_id, u.status as user_status, e.created_at, e.updated_at
           from employees e
           left join employees m on m.id = e.manager_id
           left join positions mp on mp.id = m.position_id
           left join users u on u.employee_id = e.id
          where e.id = $1",
    )
    .bind(id)
    .fetch_one(&mut *conn)
    .await?;
    let salary_visible = id == actor.me()
        || actor.has("employees:write:all")
        || actor.has("payroll:read:all")
        || actor.has("payroll:prepare");
    let manager = match (summary.manager_id, extra.manager_name, extra.manager_title) {
        (Some(mid), Some(name), Some(title)) => Some(ManagerRef {
            id: mid,
            name,
            title,
        }),
        _ => None,
    };
    Ok(EmployeeDetail {
        summary,
        pay_grade: extra.pay_grade,
        base_salary: salary_visible.then_some(extra.base_salary),
        currency: extra.currency.trim().to_string(),
        termination_date: extra.termination_date,
        manager,
        direct_reports_count: extra.direct_reports_count,
        user_id: extra.user_id,
        user_status: extra.user_status,
        created_at: extra.created_at,
        updated_at: extra.updated_at,
    })
}

#[utoipa::path(get, path = "/api/v1/employees/{id}", tag = "org", security(("bearer" = [])),
    responses((status = 200, body = EmployeeDetail), (status = 404, body = Problem)))]
pub async fn get_employee(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<EmployeeDetail>> {
    let scope = actor.scope_filter("employees:read")?;
    let mut conn = state.pool.acquire().await?;
    service::load_in_scope(&mut conn, &scope, id).await?;
    Ok(Json(employee_detail(&mut conn, &actor, id).await?))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct CreateEmployee {
    #[validate(length(min = 1, max = 80))]
    pub first_name: String,
    #[validate(length(min = 1, max = 80))]
    pub last_name: String,
    #[validate(email)]
    pub email: String,
    #[validate(length(max = 40))]
    pub phone: Option<String>,
    pub position_id: Uuid,
    pub department_id: Uuid,
    pub manager_id: Uuid,
    pub hire_date: NaiveDate,
    pub employment_type: Option<String>,
    #[validate(length(max = 80))]
    pub site: Option<String>,
    #[validate(length(max = 20))]
    pub pay_grade: Option<String>,
    pub base_salary: Option<Decimal>,
    /// Role keys; defaults to the role matching the position level plus baseline.
    pub roles: Option<Vec<String>>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CreatedEmployee {
    pub employee: EmployeeDetail,
    pub user_id: Uuid,
    /// Shown once; the user must change it on first login.
    pub temporary_password: String,
}

#[utoipa::path(post, path = "/api/v1/employees", tag = "org", security(("bearer" = [])),
    request_body = CreateEmployee, responses((status = 201, body = CreatedEmployee)))]
pub async fn create_employee(
    State(state): State<AppState>,
    actor: Actor,
    ValidatedJson(body): ValidatedJson<CreateEmployee>,
) -> ApiResult<(StatusCode, Json<CreatedEmployee>)> {
    actor.require("employees:write:all")?;
    let employment_type = body
        .employment_type
        .clone()
        .unwrap_or_else(|| "full_time".to_string());
    if !["full_time", "part_time", "contract"].contains(&employment_type.as_str()) {
        return Err(ApiError::validation(
            "employment_type",
            "must be full_time, part_time or contract",
        ));
    }
    if body.base_salary.is_some_and(|s| s < Decimal::ZERO) {
        return Err(ApiError::validation("base_salary", "must not be negative"));
    }
    let mut tx = state.pool.begin().await?;
    let level: i16 = sqlx::query_scalar("select level from positions where id = $1")
        .bind(body.position_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| ApiError::validation("position_id", "unknown position"))?;
    let employee_no = service::next_employee_no(&mut tx).await?;
    let employee_id: Uuid = sqlx::query_scalar(
        "insert into employees (employee_no, first_name, last_name, email, phone, position_id, department_id,
                                manager_id, hire_date, employment_type, site, pay_grade, base_salary)
         values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13) returning id",
    )
    .bind(&employee_no)
    .bind(&body.first_name)
    .bind(&body.last_name)
    .bind(&body.email)
    .bind(&body.phone)
    .bind(body.position_id)
    .bind(body.department_id)
    .bind(body.manager_id)
    .bind(body.hire_date)
    .bind(&employment_type)
    .bind(&body.site)
    .bind(&body.pay_grade)
    .bind(body.base_salary.unwrap_or(Decimal::ZERO))
    .fetch_one(&mut *tx)
    .await?;
    let temporary_password = crate::auth::password::generate_temporary();
    let hash = crate::auth::password::hash_async(temporary_password.clone()).await?;
    let user_id: Uuid = sqlx::query_scalar(
        "insert into users (employee_id, email, password_hash, must_change_password) values ($1, $2, $3, true) returning id",
    )
    .bind(employee_id)
    .bind(&body.email)
    .bind(hash)
    .fetch_one(&mut *tx)
    .await?;
    let roles = body
        .roles
        .clone()
        .unwrap_or_else(|| vec![service::role_for_level(level).to_string()]);
    service::set_roles(&mut tx, user_id, Some(actor.user_id()), &roles).await?;
    service::init_leave_balances(&mut tx, employee_id).await?;
    let after = audit::snapshot(&mut tx, "employees", employee_id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "employee.create",
        "employee",
        Some(employee_id),
        None,
        after,
    )
    .await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "user.create",
        "user",
        Some(user_id),
        None,
        Some(serde_json::json!({"email": body.email, "roles": roles})),
    )
    .await?;
    let detail = employee_detail(&mut tx, &actor, employee_id).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(CreatedEmployee {
            employee: detail,
            user_id,
            temporary_password,
        }),
    ))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct UpdateEmployee {
    #[validate(length(min = 1, max = 80))]
    pub first_name: Option<String>,
    #[validate(length(min = 1, max = 80))]
    pub last_name: Option<String>,
    #[validate(length(max = 40))]
    pub phone: Option<String>,
    pub position_id: Option<Uuid>,
    pub department_id: Option<Uuid>,
    /// Re-parents the employee; the whole subtree follows.
    pub manager_id: Option<Uuid>,
    /// `active`, `on_leave` or `suspended`; use the terminate endpoint otherwise.
    pub status: Option<String>,
    pub employment_type: Option<String>,
    #[validate(length(max = 80))]
    pub site: Option<String>,
    #[validate(length(max = 20))]
    pub pay_grade: Option<String>,
    pub base_salary: Option<Decimal>,
}

#[utoipa::path(patch, path = "/api/v1/employees/{id}", tag = "org", security(("bearer" = [])),
    request_body = UpdateEmployee, responses((status = 200, body = EmployeeDetail), (status = 409, body = Problem)))]
pub async fn update_employee(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<UpdateEmployee>,
) -> ApiResult<Json<EmployeeDetail>> {
    let write_scope = actor.scope("employees:write").ok_or_else(|| {
        ApiError::forbidden("requires employees:write:subtree or employees:write:all")
    })?;
    let filter = actor.filter(write_scope);
    let mut tx = state.pool.begin().await?;
    let target = service::load_in_scope(&mut tx, &filter, id).await?;
    if write_scope == Scope::Subtree && target.id == actor.me() {
        return Err(ApiError::forbidden("you cannot edit your own record"));
    }
    if let Some(status) = &body.status {
        if !["active", "on_leave", "suspended"].contains(&status.as_str()) {
            return Err(ApiError::validation(
                "status",
                "must be active, on_leave or suspended",
            ));
        }
        if target.status == "terminated" {
            return Err(ApiError::transition("terminated", status));
        }
    }
    if let Some(et) = &body.employment_type {
        if !["full_time", "part_time", "contract"].contains(&et.as_str()) {
            return Err(ApiError::validation(
                "employment_type",
                "must be full_time, part_time or contract",
            ));
        }
    }
    if let Some(new_manager) = body.manager_id {
        if new_manager == id {
            return Err(ApiError::validation(
                "manager_id",
                "an employee cannot manage themselves",
            ));
        }
        // Subtree writers may only move people to managers they also oversee.
        service::load_in_scope(&mut tx, &filter, new_manager)
            .await
            .map_err(|_| ApiError::validation("manager_id", "manager not found in your scope"))?;
    }
    if body.base_salary.is_some()
        && !(actor.has("employees:write:all") || actor.has("payroll:prepare"))
    {
        return Err(ApiError::forbidden(
            "changing salary requires employees:write:all",
        ));
    }
    let before = audit::snapshot(&mut tx, "employees", id).await?;
    let mut qb: QueryBuilder<Postgres> =
        QueryBuilder::new("update employees set updated_at = now()");
    if let Some(v) = &body.first_name {
        qb.push(", first_name = ").push_bind(v.clone());
    }
    if let Some(v) = &body.last_name {
        qb.push(", last_name = ").push_bind(v.clone());
    }
    if let Some(v) = &body.phone {
        qb.push(", phone = ").push_bind(v.clone());
    }
    if let Some(v) = body.position_id {
        qb.push(", position_id = ").push_bind(v);
    }
    if let Some(v) = body.department_id {
        qb.push(", department_id = ").push_bind(v);
    }
    if let Some(v) = body.manager_id {
        qb.push(", manager_id = ").push_bind(v);
    }
    if let Some(v) = &body.status {
        qb.push(", status = ").push_bind(v.clone());
    }
    if let Some(v) = &body.employment_type {
        qb.push(", employment_type = ").push_bind(v.clone());
    }
    if let Some(v) = &body.site {
        qb.push(", site = ").push_bind(v.clone());
    }
    if let Some(v) = &body.pay_grade {
        qb.push(", pay_grade = ").push_bind(v.clone());
    }
    if let Some(v) = body.base_salary {
        qb.push(", base_salary = ").push_bind(v);
    }
    qb.push(" where id = ").push_bind(id);
    qb.build().execute(&mut *tx).await?;
    let after = audit::snapshot(&mut tx, "employees", id).await?;
    audit::record(
        &mut tx,
        &actor.audit(),
        "employee.update",
        "employee",
        Some(id),
        before,
        after,
    )
    .await?;
    if let Some(user_id) =
        sqlx::query_scalar::<_, Uuid>("select id from users where employee_id = $1")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
    {
        state.principals.evict(user_id).await;
    }
    let detail = employee_detail(&mut tx, &actor, id).await?;
    tx.commit().await?;
    Ok(Json(detail))
}

#[derive(Debug, Deserialize, Validate, ToSchema)]
pub struct TerminateEmployee {
    pub termination_date: NaiveDate,
    /// Where the direct reports go; defaults to the terminated employee's manager.
    pub reassign_reports_to: Option<Uuid>,
}

#[utoipa::path(post, path = "/api/v1/employees/{id}/terminate", tag = "org", security(("bearer" = [])),
    request_body = TerminateEmployee, responses((status = 200, body = EmployeeDetail)))]
pub async fn terminate_employee(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
    ValidatedJson(body): ValidatedJson<TerminateEmployee>,
) -> ApiResult<Json<EmployeeDetail>> {
    actor.require("employees:write:all")?;
    if id == actor.me() {
        return Err(ApiError::forbidden("you cannot terminate yourself"));
    }
    let mut tx = state.pool.begin().await?;
    let target = service::load_core(&mut tx, id)
        .await?
        .ok_or_else(|| ApiError::not_found("employee"))?;
    if target.status == "terminated" {
        return Err(ApiError::transition("terminated", "terminated"));
    }
    let Some(fallback_manager) = target.manager_id else {
        return Err(ApiError::conflict(
            "the chief executive cannot be terminated through the API",
        ));
    };
    let new_manager = body.reassign_reports_to.unwrap_or(fallback_manager);
    if new_manager == id {
        return Err(ApiError::validation(
            "reassign_reports_to",
            "cannot be the terminated employee",
        ));
    }
    let new_manager_row = service::load_core(&mut tx, new_manager)
        .await?
        .ok_or_else(|| ApiError::validation("reassign_reports_to", "unknown employee"))?;
    if new_manager_row.status == "terminated"
        || new_manager_row
            .path
            .starts_with(&format!("{}.", target.path))
    {
        return Err(ApiError::validation(
            "reassign_reports_to",
            "must be an active employee outside the terminated subtree",
        ));
    }
    let before = audit::snapshot(&mut tx, "employees", id).await?;
    let moved = sqlx::query(
        "update employees set manager_id = $2 where manager_id = $1 and status <> 'terminated'",
    )
    .bind(id)
    .bind(new_manager)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    sqlx::query("update employees set status = 'terminated', termination_date = $2 where id = $1")
        .bind(id)
        .bind(body.termination_date)
        .execute(&mut *tx)
        .await?;
    let user_id: Option<Uuid> = sqlx::query_scalar(
        "update users set status = 'disabled', token_version = token_version + 1 where employee_id = $1 returning id",
    )
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some(uid) = user_id {
        sqlx::query("update refresh_tokens set revoked_at = now() where user_id = $1 and revoked_at is null")
            .bind(uid)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("update work_orders set assigned_to = null where assigned_to = $1 and status in ('open','blocked')")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let after = audit::snapshot(&mut tx, "employees", id).await?;
    audit::record(&mut tx, &actor.audit(), "employee.terminate", "employee", Some(id), before,
        after.map(|a| serde_json::json!({"employee": a, "reports_reassigned_to": new_manager, "reports_moved": moved}))).await?;
    let detail = employee_detail(&mut tx, &actor, id).await?;
    tx.commit().await?;
    if let Some(uid) = user_id {
        state.principals.evict(uid).await;
    }
    Ok(Json(detail))
}

#[utoipa::path(get, path = "/api/v1/employees/{id}/reports", tag = "org", security(("bearer" = [])),
    responses((status = 200, body = Vec<EmployeeSummary>)))]
pub async fn direct_reports(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<EmployeeSummary>>> {
    let scope = actor.scope_filter("employees:read")?;
    let mut conn = state.pool.acquire().await?;
    // Visibility is the caller's `employees:read` scope, exactly as it is for
    // `GET /employees/{id}`. `org:read` must NOT widen it: every active user holds
    // that permission through the baseline role, so accepting it here would let any
    // employee enumerate any manager's reports together with their contact details.
    // The org chart is served separately by `/org/tree`, which carries names, titles
    // and reporting lines only.
    let visible = service::load_core(&mut conn, id)
        .await?
        .is_some_and(|e| scope.contains(e.id, &e.path, e.department_id) || e.id == actor.me());
    if !visible {
        return Err(ApiError::not_found("employee"));
    }
    let rows: Vec<EmployeeSummary> = sqlx::query_as(&format!(
        "{EMPLOYEE_SELECT} e.manager_id = $1 and e.status <> 'terminated' order by e.last_name, e.first_name"
    ))
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Json(rows))
}

#[utoipa::path(get, path = "/api/v1/employees/{id}/chain", tag = "org", security(("bearer" = [])),
    responses((status = 200, body = Vec<ChainEntry>)))]
pub async fn chain(
    State(state): State<AppState>,
    actor: Actor,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<ChainEntry>>> {
    actor.require("org:read")?;
    let mut conn = state.pool.acquire().await?;
    let target = service::load_core(&mut conn, id)
        .await?
        .ok_or_else(|| ApiError::not_found("employee"))?;
    Ok(Json(
        service::chain_of_command(&mut conn, &target.path, target.id).await?,
    ))
}

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/org/tree", get(tree))
        .route("/org/departments", get(departments))
        .route("/org/positions", get(positions))
        .route("/employees", get(list_employees).post(create_employee))
        .route("/employees/:id", get(get_employee).patch(update_employee))
        .route("/employees/:id/terminate", post(terminate_employee))
        .route("/employees/:id/reports", get(direct_reports))
        .route("/employees/:id/chain", get(chain))
}

#[derive(OpenApi)]
#[openapi(paths(
    tree,
    departments,
    positions,
    list_employees,
    get_employee,
    create_employee,
    update_employee,
    terminate_employee,
    direct_reports,
    chain
))]
pub struct OrgApi;
