//! Shared harness for the integration suite.
//!
//! Every test gets its own throwaway Postgres database, the real migrations, a real
//! [`AppState`] and the real router. Requests are driven in process with
//! `tower::ServiceExt::oneshot`, so nothing binds a port and the whole suite is safe
//! to run in parallel.
//!
//! Configuration is built as a [`Config`] value rather than through
//! `Config::from_env`, because the process environment is shared by every test in a
//! binary and each test needs a different `DATABASE_URL`. Only the base
//! `DATABASE_URL` is read from the environment, and only to find the server.

#![allow(dead_code)]

use std::future::Future;
use std::net::SocketAddr;
use std::sync::OnceLock;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use axum::Router;
use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use sqlx::{Connection, PgConnection, PgPool};
use tower::ServiceExt;
use uuid::Uuid;

use bowline_api::config::{LogFormat, S3Config, SeedConfig};
use bowline_api::{db, http, AppState, Config};

/// The router is one deeply nested tower type per route and per layer. Building it
/// needs far more stack than a test thread is given by default, so the harness runs
/// every test on a thread sized like the one `main` reserves.
const STACK_SIZE: usize = 32 * 1024 * 1024;

/// Password every fixture user is created with.
pub const PASSWORD: &str = "Bowline!2026";
/// Failed logins the harness allows before an account locks.
pub const LOGIN_MAX_FAILURES: i32 = 3;

/// Argon2id is deliberately slow. The fixture reuses one hash of [`PASSWORD`] for
/// every user in the binary rather than paying for it once per account per test.
fn password_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| bowline_api::auth::password::hash(PASSWORD).expect("hashing the password"))
}

fn base_database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://bowline_app:bowline_app_dev@localhost:5432/bowline".into())
}

/// Swaps the database name in a connection string, keeping any query parameters.
fn with_database(url: &str, database: &str) -> String {
    let (base, query) = match url.split_once('?') {
        Some((base, query)) => (base, Some(query)),
        None => (url, None),
    };
    let cut = base.rfind('/').expect("a connection string with a path");
    let mut out = format!("{}/{}", &base[..cut], database);
    if let Some(query) = query {
        out.push('?');
        out.push_str(query);
    }
    out
}

/// Runs an async test body on a thread with enough stack to build the router.
pub fn run<F, Fut>(body: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: Future<Output = ()>,
{
    let thread = std::thread::Builder::new()
        .stack_size(STACK_SIZE)
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .thread_stack_size(STACK_SIZE)
                .enable_all()
                .build()
                .expect("building the test runtime");
            runtime.block_on(body());
        })
        .expect("spawning the test thread");
    if let Err(panic) = thread.join() {
        std::panic::resume_unwind(panic);
    }
}

/// One seeded person: the employee record and the user account behind it.
#[derive(Debug, Clone)]
pub struct Person {
    pub employee_id: Uuid,
    pub user_id: Uuid,
    pub email: String,
}

/// The small organisation every test starts from.
#[derive(Debug, Clone)]
pub struct Fixture {
    pub ceo: Person,
    pub cfo: Person,
    pub warehouse_manager: Person,
    pub dock_supervisor: Person,
    pub driver: Person,
    pub dock_worker: Person,
    pub accountant: Person,
    pub support_agent: Person,
    pub it_admin: Person,
    pub dispatcher: Person,
    /// Head of the unrelated department subtree.
    pub sales_manager: Person,
    /// Sits under [`Fixture::sales_manager`], so warehouse scopes must exclude them.
    pub sales_rep: Person,
    pub dept_executive: Uuid,
    pub dept_operations: Uuid,
    pub dept_warehousing: Uuid,
    pub dept_finance: Uuid,
    pub dept_service_desk: Uuid,
    pub dept_sales: Uuid,
}

impl Fixture {
    /// Everyone the fixture creates, in no particular order.
    pub fn everyone(&self) -> Vec<&Person> {
        vec![
            &self.ceo,
            &self.cfo,
            &self.warehouse_manager,
            &self.dock_supervisor,
            &self.driver,
            &self.dock_worker,
            &self.accountant,
            &self.support_agent,
            &self.it_admin,
            &self.dispatcher,
            &self.sales_manager,
            &self.sales_rep,
        ]
    }

    pub fn headcount(&self) -> usize {
        self.everyone().len()
    }
}

/// A running application: a private database, the real router, and the fixture.
pub struct TestApp {
    pub app: Router,
    pub pool: PgPool,
    pub fx: Fixture,
    pub db_name: String,
    admin_url: String,
}

impl TestApp {
    /// Creates the database, migrates it, seeds the fixture and builds the router.
    pub async fn start() -> TestApp {
        let base = base_database_url();
        let admin_url = with_database(&base, "postgres");
        let db_name = format!("bowline_test_{}", Uuid::new_v4().simple());

        let mut admin = PgConnection::connect(&admin_url)
            .await
            .expect("connecting to the postgres maintenance database");
        sqlx::raw_sql(&format!("create database \"{db_name}\""))
            .execute(&mut admin)
            .await
            .expect("creating the test database");
        admin.close().await.ok();

        let database_url = with_database(&base, &db_name);
        let pool = db::connect(&database_url, 4)
            .await
            .expect("connecting to the test database");
        db::migrate(&pool).await.expect("applying the migrations");

        let fx = seed(&pool).await;
        let config = test_config(database_url);
        let state = AppState::build(config, pool.clone(), None).await;
        let app = http::router(state, None);

        TestApp {
            app,
            pool,
            fx,
            db_name,
            admin_url,
        }
    }

    // ---------------------------------------------------------------------
    // HTTP
    // ---------------------------------------------------------------------

    /// Sends one request through the router and reads the JSON body back.
    pub async fn send(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            builder = builder.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let request = match body {
            Some(value) => builder
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(serde_json::to_vec(&value).expect("json body")))
                .expect("building the request"),
            None => builder.body(Body::empty()).expect("building the request"),
        };
        let response = self
            .app
            .clone()
            .oneshot(request)
            .await
            .expect("the router always answers");
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .expect("reading the response body");
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, value)
    }

    pub async fn get(&self, path: &str, token: &str) -> (StatusCode, Value) {
        self.send(Method::GET, path, Some(token), None).await
    }

    pub async fn post(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        self.send(Method::POST, path, Some(token), Some(body)).await
    }

    pub async fn post_empty(&self, path: &str, token: &str) -> (StatusCode, Value) {
        self.send(Method::POST, path, Some(token), Some(json!({})))
            .await
    }

    pub async fn patch(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        self.send(Method::PATCH, path, Some(token), Some(body))
            .await
    }

    pub async fn put(&self, path: &str, token: &str, body: Value) -> (StatusCode, Value) {
        self.send(Method::PUT, path, Some(token), Some(body)).await
    }

    /// Anonymous POST, for the auth routes.
    pub async fn post_anon(&self, path: &str, body: Value) -> (StatusCode, Value) {
        self.send(Method::POST, path, None, Some(body)).await
    }

    // ---------------------------------------------------------------------
    // Sessions
    // ---------------------------------------------------------------------

    /// Logs a person in and returns their access token.
    pub async fn token(&self, person: &Person) -> String {
        let (status, body) = self.login(&person.email, PASSWORD).await;
        assert_eq!(status, StatusCode::OK, "login failed: {body}");
        body["access_token"]
            .as_str()
            .expect("an access token")
            .to_string()
    }

    /// Logs a person in and returns both tokens.
    pub async fn session(&self, person: &Person) -> (String, String) {
        let (status, body) = self.login(&person.email, PASSWORD).await;
        assert_eq!(status, StatusCode::OK, "login failed: {body}");
        (
            body["access_token"].as_str().expect("access").to_string(),
            body["refresh_token"].as_str().expect("refresh").to_string(),
        )
    }

    pub async fn login(&self, email: &str, password: &str) -> (StatusCode, Value) {
        self.post_anon(
            "/api/v1/auth/login",
            json!({"email": email, "password": password}),
        )
        .await
    }

    // ---------------------------------------------------------------------
    // Convenience builders used by more than one suite
    // ---------------------------------------------------------------------

    /// Creates a customer through the API and returns its id.
    pub async fn customer(&self, token: &str, code: &str) -> Uuid {
        let (status, body) = self
            .post(
                "/api/v1/ops/customers",
                token,
                json!({"code": code, "name": format!("{code} Freight Ltd")}),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "customer not created: {body}");
        uuid(&body["id"])
    }

    /// Creates a draft shipment through the API and returns its id.
    pub async fn shipment(&self, token: &str, customer_id: Uuid) -> Uuid {
        let (status, body) = self
            .post(
                "/api/v1/ops/shipments",
                token,
                json!({
                    "customer_id": customer_id,
                    "mode": "road",
                    "origin": {"city": "Rotterdam", "country": "NL"},
                    "destination": {"city": "Hamburg", "country": "DE"},
                    "cargo_description": "Palletised machine parts",
                    "pieces": 12,
                    "weight_kg": "4200.00"
                }),
            )
            .await;
        assert_eq!(status, StatusCode::CREATED, "shipment not created: {body}");
        uuid(&body["id"])
    }

    /// The fiscal period covering today, read through the API.
    pub async fn current_period(&self, token: &str) -> Uuid {
        let today = chrono::Utc::now().date_naive();
        let (status, body) = self
            .get(
                &format!("/api/v1/finance/periods?year={}&per_page=100", today.year()),
                token,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "periods not readable: {body}");
        let month = today.month() as i64;
        let period = body["items"]
            .as_array()
            .expect("items")
            .iter()
            .find(|p| p["month"].as_i64() == Some(month))
            .expect("a fiscal period covering today");
        uuid(&period["id"])
    }

    /// Rows in the notification outbox addressed to one employee.
    pub async fn notifications_for(&self, employee_id: Uuid) -> i64 {
        sqlx::query_scalar("select count(*) from notifications where recipient_id = $1")
            .bind(employee_id)
            .fetch_one(&self.pool)
            .await
            .expect("counting notifications")
    }

    /// The ltree reporting path stored for an employee.
    pub async fn path_of(&self, employee_id: Uuid) -> String {
        sqlx::query_scalar("select path::text from employees where id = $1")
            .bind(employee_id)
            .fetch_one(&self.pool)
            .await
            .expect("reading the reporting path")
    }
}

impl Drop for TestApp {
    fn drop(&mut self) {
        // Drop cannot await, and this may run while a test is unwinding, so the
        // clean-up gets its own thread and its own runtime. `with (force)` closes
        // the pool's connections for us.
        let admin_url = self.admin_url.clone();
        let db_name = self.db_name.clone();
        let cleanup = std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            else {
                return;
            };
            runtime.block_on(async {
                if let Ok(mut conn) = PgConnection::connect(&admin_url).await {
                    let _ = sqlx::raw_sql(&format!(
                        "drop database if exists \"{db_name}\" with (force)"
                    ))
                    .execute(&mut conn)
                    .await;
                    conn.close().await.ok();
                }
            });
        });
        let _ = cleanup.join();
    }
}

use chrono::Datelike;

fn test_config(database_url: String) -> Config {
    Config {
        database_url,
        database_max_connections: 4,
        database_migrate_on_start: true,
        // Redis is an accelerator, never a dependency: the tests use the in-process
        // principal cache so nothing outside Postgres has to be running.
        redis_url: None,
        api_bind: SocketAddr::from(([127, 0, 0, 1], 0)),
        api_public_url: "http://localhost:8080".to_string(),
        cors_origins: Vec::new(),
        jwt_secret: "integration-test-secret-0123456789abcdef".to_string(),
        jwt_issuer: "bowline-test".to_string(),
        access_token_ttl_seconds: 900,
        refresh_token_ttl_seconds: 3_600,
        login_max_failures: LOGIN_MAX_FAILURES,
        login_lockout_seconds: 900,
        // High enough that no test trips the limiter by accident.
        rate_limit_per_minute: 10_000,
        invoice_approval_threshold: Decimal::from(50_000),
        // Port 1 refuses instantly, so the billing and analytics calls fail open
        // without waiting for a timeout and without reaching a real service.
        billing_url: "http://127.0.0.1:1".to_string(),
        analytics_url: "http://127.0.0.1:1".to_string(),
        internal_service_token: "integration-test-internal-token".to_string(),
        log_format: LogFormat::Pretty,
        s3: S3Config {
            endpoint: Some("http://127.0.0.1:1".to_string()),
            region: "us-east-1".to_string(),
            bucket_documents: "bowline-test-documents".to_string(),
            bucket_pdfs: "bowline-test-pdfs".to_string(),
            access_key_id: "test".to_string(),
            secret_access_key: "test".to_string(),
            force_path_style: true,
            presign_ttl_seconds: 900,
        },
        seed: SeedConfig {
            password: PASSWORD.to_string(),
            skip_password_change: true,
            random_seed: 42,
        },
    }
}

// -------------------------------------------------------------------------
// Fixture
// -------------------------------------------------------------------------

async fn department(pool: &PgPool, code: &str, name: &str, parent: Option<Uuid>) -> Uuid {
    sqlx::query_scalar(
        "insert into departments (code, name, parent_id) values ($1::citext, $2, $3) returning id",
    )
    .bind(code)
    .bind(name)
    .bind(parent)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| panic!("creating department {code}: {e}"))
}

async fn position(pool: &PgPool, code: &str, title: &str, level: i16, department: Uuid) -> Uuid {
    sqlx::query_scalar(
        "insert into positions (code, title, level, department_id, is_people_manager)
         values ($1::citext, $2, $3, $4, $5) returning id",
    )
    .bind(code)
    .bind(title)
    .bind(level)
    .bind(department)
    .bind(level <= 5)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| panic!("creating position {code}: {e}"))
}

#[allow(clippy::too_many_arguments)]
async fn hire(
    pool: &PgPool,
    number: &str,
    first: &str,
    last: &str,
    local_part: &str,
    position_id: Uuid,
    department_id: Uuid,
    manager_id: Option<Uuid>,
    roles: &[&str],
) -> Person {
    let email = format!("{local_part}@bowline.test");
    let hire_date = NaiveDate::from_ymd_opt(2021, 1, 4).expect("a valid hire date");
    let employee_id: Uuid = sqlx::query_scalar(
        "insert into employees (employee_no, first_name, last_name, email, position_id,
                                department_id, manager_id, hire_date, site, base_salary)
         values ($1, $2, $3, $4::citext, $5, $6, $7, $8, 'Head Office', 60000)
         returning id",
    )
    .bind(number)
    .bind(first)
    .bind(last)
    .bind(&email)
    .bind(position_id)
    .bind(department_id)
    .bind(manager_id)
    .bind(hire_date)
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| panic!("hiring {email}: {e}"));

    let user_id: Uuid = sqlx::query_scalar(
        "insert into users (employee_id, email, password_hash, status, must_change_password,
                            token_version)
         values ($1, $2::citext, $3, 'active', false, 1) returning id",
    )
    .bind(employee_id)
    .bind(&email)
    .bind(password_hash())
    .fetch_one(pool)
    .await
    .unwrap_or_else(|e| panic!("creating the user for {email}: {e}"));

    let mut conn = pool.acquire().await.expect("a connection for role grants");
    let roles: Vec<String> = roles.iter().map(|r| (*r).to_string()).collect();
    bowline_api::org::service::set_roles(&mut conn, user_id, None, &roles)
        .await
        .unwrap_or_else(|e| panic!("granting roles to {email}: {e:?}"));

    Person {
        employee_id,
        user_id,
        email,
    }
}

/// Seeds a small organisation: one chain from the chief executive down to the dock,
/// a finance branch, a service desk, and an unrelated commercial subtree that the
/// warehouse scopes must never see.
async fn seed(pool: &PgPool) -> Fixture {
    let executive = department(pool, "T_EXEC", "Executive Office", None).await;
    let operations = department(pool, "T_OPS", "Operations", Some(executive)).await;
    let warehousing = department(pool, "T_WH", "Warehousing", Some(operations)).await;
    let finance = department(pool, "T_FIN", "Finance", Some(executive)).await;
    let technology = department(pool, "T_TECH", "Technology", Some(executive)).await;
    let service_desk = department(pool, "T_DESK", "Service Desk", Some(technology)).await;
    let commercial = department(pool, "T_COMM", "Commercial", Some(executive)).await;
    let sales = department(pool, "T_SALES", "Sales", Some(commercial)).await;

    let p_ceo = position(pool, "T_P_CEO", "Chief Executive Officer", 1, executive).await;
    let p_cfo = position(pool, "T_P_CFO", "Chief Financial Officer", 2, finance).await;
    let p_wm = position(pool, "T_P_WM", "Warehouse Manager", 4, warehousing).await;
    let p_sup = position(pool, "T_P_SUP", "Dock Supervisor", 5, warehousing).await;
    let p_drv = position(pool, "T_P_DRV", "Driver", 7, warehousing).await;
    let p_dock = position(pool, "T_P_DCK", "Dock Worker", 7, warehousing).await;
    let p_acc = position(pool, "T_P_ACC", "Accountant", 6, finance).await;
    let p_agent = position(pool, "T_P_AGT", "Support Agent", 6, service_desk).await;
    let p_ita = position(pool, "T_P_ITA", "Platform Engineering Lead", 4, technology).await;
    let p_disp = position(pool, "T_P_DSP", "Dispatch Supervisor", 5, operations).await;
    let p_sm = position(pool, "T_P_SM", "Sales Manager", 4, sales).await;
    let p_rep = position(pool, "T_P_REP", "Account Executive", 6, sales).await;

    let ceo = hire(
        pool,
        "EMP-000001",
        "Ada",
        "Kestrel",
        "ceo",
        p_ceo,
        executive,
        None,
        &["executive"],
    )
    .await;
    let cfo = hire(
        pool,
        "EMP-000002",
        "Bruno",
        "Halvard",
        "cfo",
        p_cfo,
        finance,
        Some(ceo.employee_id),
        &["finance_admin", "executive"],
    )
    .await;
    let warehouse_manager = hire(
        pool,
        "EMP-000003",
        "Cora",
        "Lindqvist",
        "warehouse.manager",
        p_wm,
        warehousing,
        Some(ceo.employee_id),
        &["manager"],
    )
    .await;
    let dock_supervisor = hire(
        pool,
        "EMP-000004",
        "Dev",
        "Okonkwo",
        "dock.supervisor",
        p_sup,
        warehousing,
        Some(warehouse_manager.employee_id),
        &["supervisor"],
    )
    .await;
    let driver = hire(
        pool,
        "EMP-000005",
        "Elin",
        "Marchetti",
        "driver",
        p_drv,
        warehousing,
        Some(dock_supervisor.employee_id),
        &["field_worker"],
    )
    .await;
    let dock_worker = hire(
        pool,
        "EMP-000006",
        "Fen",
        "Achterberg",
        "dock.worker",
        p_dock,
        warehousing,
        Some(dock_supervisor.employee_id),
        &["field_worker"],
    )
    .await;
    let accountant = hire(
        pool,
        "EMP-000007",
        "Gita",
        "Ravenna",
        "accountant",
        p_acc,
        finance,
        Some(cfo.employee_id),
        &["accountant"],
    )
    .await;
    let support_agent = hire(
        pool,
        "EMP-000008",
        "Hal",
        "Brennan",
        "support.agent",
        p_agent,
        service_desk,
        Some(ceo.employee_id),
        &["support_agent"],
    )
    .await;
    let it_admin = hire(
        pool,
        "EMP-000009",
        "Iris",
        "Vandermeer",
        "it.admin",
        p_ita,
        technology,
        Some(ceo.employee_id),
        &["it_admin"],
    )
    .await;
    let dispatcher = hire(
        pool,
        "EMP-000010",
        "Jonas",
        "Petrakis",
        "dispatcher",
        p_disp,
        operations,
        Some(ceo.employee_id),
        &["dispatcher"],
    )
    .await;
    let sales_manager = hire(
        pool,
        "EMP-000011",
        "Kira",
        "Solberg",
        "sales.manager",
        p_sm,
        sales,
        Some(ceo.employee_id),
        &["manager"],
    )
    .await;
    let sales_rep = hire(
        pool,
        "EMP-000012",
        "Luc",
        "Ferreira",
        "sales.rep",
        p_rep,
        sales,
        Some(sales_manager.employee_id),
        &["staff"],
    )
    .await;

    Fixture {
        ceo,
        cfo,
        warehouse_manager,
        dock_supervisor,
        driver,
        dock_worker,
        accountant,
        support_agent,
        it_admin,
        dispatcher,
        sales_manager,
        sales_rep,
        dept_executive: executive,
        dept_operations: operations,
        dept_warehousing: warehousing,
        dept_finance: finance,
        dept_service_desk: service_desk,
        dept_sales: sales,
    }
}

// -------------------------------------------------------------------------
// Assertion helpers
// -------------------------------------------------------------------------

/// The RFC 7807 `code` of a problem document.
pub fn code(body: &Value) -> &str {
    body["code"].as_str().unwrap_or("<no code field>")
}

pub fn uuid(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().expect("a uuid string")).expect("a well formed uuid")
}

/// The `items` array of a list envelope.
pub fn items(body: &Value) -> &Vec<Value> {
    body["items"].as_array().expect("a list envelope")
}

/// The ids in an `items` array.
pub fn ids(body: &Value) -> Vec<Uuid> {
    items(body).iter().map(|item| uuid(&item["id"])).collect()
}

/// The `total` of a list envelope.
pub fn total(body: &Value) -> i64 {
    body["total"].as_i64().expect("a total")
}
