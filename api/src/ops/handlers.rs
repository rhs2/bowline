//! Operations router: customers, fleet, shipments, work orders and inventory.

use axum::routing::{get, patch, post};
use axum::Router;
use utoipa::OpenApi;

use crate::ops::{reference, shipments, work};
use crate::state::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/ops/customers",
            get(reference::list_customers).post(reference::create_customer),
        )
        .route(
            "/ops/customers/:id",
            get(reference::get_customer).patch(reference::update_customer),
        )
        .route(
            "/ops/carriers",
            get(reference::list_carriers).post(reference::create_carrier),
        )
        .route(
            "/ops/carriers/:id",
            get(reference::get_carrier).patch(reference::update_carrier),
        )
        .route(
            "/ops/sites",
            get(reference::list_sites).post(reference::create_site),
        )
        .route(
            "/ops/sites/:id",
            get(reference::get_site).patch(reference::update_site),
        )
        .route(
            "/ops/vehicles",
            get(reference::list_vehicles).post(reference::create_vehicle),
        )
        .route(
            "/ops/vehicles/:id",
            get(reference::get_vehicle).patch(reference::update_vehicle),
        )
        .route(
            "/ops/shipments",
            get(shipments::list_shipments).post(shipments::create_shipment),
        )
        .route(
            "/ops/shipments/:id",
            get(shipments::get_shipment).patch(shipments::update_shipment),
        )
        .route(
            "/ops/shipments/:id/transition",
            post(shipments::transition_shipment),
        )
        .route("/ops/shipments/:id/legs", post(shipments::create_leg))
        .route(
            "/ops/shipments/:id/legs/:leg_id",
            patch(shipments::update_leg),
        )
        .route("/ops/shipments/:id/events", post(shipments::create_event))
        .route(
            "/ops/shipments/:id/documents",
            post(shipments::confirm_shipment_document),
        )
        .route(
            "/ops/shipments/:id/documents/presign",
            post(shipments::presign_shipment_document),
        )
        .route(
            "/ops/shipments/:id/documents/:doc_id/download",
            get(shipments::download_shipment_document),
        )
        .route(
            "/ops/work-orders",
            get(work::list_work_orders).post(work::create_work_order),
        )
        .route(
            "/ops/work-orders/:id/status",
            post(work::update_work_order_status),
        )
        .route(
            "/ops/inventory",
            get(work::list_inventory).post(work::create_inventory_item),
        )
}

#[derive(OpenApi)]
#[openapi(paths(
    reference::list_customers,
    reference::get_customer,
    reference::create_customer,
    reference::update_customer,
    reference::list_carriers,
    reference::get_carrier,
    reference::create_carrier,
    reference::update_carrier,
    reference::list_sites,
    reference::get_site,
    reference::create_site,
    reference::update_site,
    reference::list_vehicles,
    reference::get_vehicle,
    reference::create_vehicle,
    reference::update_vehicle,
    shipments::list_shipments,
    shipments::get_shipment,
    shipments::create_shipment,
    shipments::update_shipment,
    shipments::transition_shipment,
    shipments::create_leg,
    shipments::update_leg,
    shipments::create_event,
    shipments::presign_shipment_document,
    shipments::confirm_shipment_document,
    shipments::download_shipment_document,
    work::list_work_orders,
    work::create_work_order,
    work::update_work_order_status,
    work::list_inventory,
    work::create_inventory_item
))]
pub struct OpsApi;
