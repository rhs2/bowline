//! The OpenAPI document, assembled from every module's path set.

use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};

#[derive(OpenApi)]
#[openapi(
    info(title = "Bowline API", version = "1.0.0",
         description = "Freight operations and workforce platform: identity, organisation, HR, operations, finance, communications, support and audit."),
    paths(crate::health::healthz, crate::health::readyz, crate::health::metrics),
    modifiers(&BearerAuth),
    tags(
        (name = "auth", description = "Login, token refresh, password change"),
        (name = "org", description = "Departments, positions, employees, chain of command"),
        (name = "hr", description = "Leave, shifts, attendance, documents"),
        (name = "ops", description = "Customers, fleet, shipments, work orders, inventory"),
        (name = "finance", description = "Ledger, invoices, payments, payables, expenses, payroll, reports"),
        (name = "comms", description = "Threads, messages, announcements"),
        (name = "support", description = "Service desk tickets"),
        (name = "admin", description = "Users, roles, audit log"),
        (name = "dashboard", description = "Role-aware summary"),
        (name = "platform", description = "Health, readiness, metrics")
    )
)]
pub struct ApiDoc;

struct BearerAuth;

impl Modify for BearerAuth {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "bearer",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .build(),
            ),
        );
    }
}

impl ApiDoc {
    pub fn document() -> utoipa::openapi::OpenApi {
        let mut doc = ApiDoc::openapi();
        doc.merge(crate::auth::handlers::AuthApi::openapi());
        doc.merge(crate::org::handlers::OrgApi::openapi());
        doc.merge(crate::hr::handlers::HrApi::openapi());
        doc.merge(crate::ops::handlers::OpsApi::openapi());
        doc.merge(crate::finance::handlers::FinanceApi::openapi());
        doc.merge(crate::comms::handlers::CommsApi::openapi());
        doc.merge(crate::support::handlers::SupportApi::openapi());
        doc.merge(crate::admin::handlers::AdminApi::openapi());
        doc.merge(crate::dashboard::DashboardApi::openapi());
        doc
    }
}
