//! Operations helpers: the shipment state machine, delay-risk scoring, and the
//! small lookups the handlers share.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use sqlx::PgConnection;
use uuid::Uuid;

use crate::clients::analytics::{AnalyticsClient, DelayRiskRequest};
use crate::clients::s3::sanitise_filename;
use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Transport modes shared by shipments, legs and carriers.
pub const MODES: [&str; 4] = ["sea", "air", "road", "rail"];
pub const INCOTERMS: [&str; 6] = ["EXW", "FCA", "FOB", "CIF", "DAP", "DDP"];
pub const CUSTOMER_STATUSES: [&str; 3] = ["active", "on_hold", "closed"];
pub const SITE_KINDS: [&str; 5] = ["office", "warehouse", "port", "airport", "depot"];
pub const VEHICLE_KINDS: [&str; 4] = ["truck", "van", "trailer", "forklift"];
pub const VEHICLE_STATUSES: [&str; 4] = ["available", "in_use", "maintenance", "retired"];
pub const LEG_STATUSES: [&str; 4] = ["planned", "in_progress", "completed", "cancelled"];
pub const WORK_ORDER_KINDS: [&str; 6] = [
    "loading",
    "unloading",
    "pickup",
    "delivery",
    "inspection",
    "inventory",
];
pub const WORK_ORDER_STATUSES: [&str; 5] = ["open", "in_progress", "done", "blocked", "cancelled"];
pub const DOCUMENT_KINDS: [&str; 7] = [
    "bill_of_lading",
    "air_waybill",
    "commercial_invoice",
    "packing_list",
    "customs",
    "proof_of_delivery",
    "other",
];
/// Event types the timeline endpoint accepts. `created`, `resumed` and `cancelled`
/// are written by the API itself as part of a transition.
pub const MANUAL_EVENT_TYPES: [&str; 10] = [
    "booked",
    "picked_up",
    "departed",
    "arrived",
    "customs_hold",
    "customs_cleared",
    "out_for_delivery",
    "delivered",
    "exception",
    "note",
];
/// Every state a shipment may hold.
pub const SHIPMENT_STATUSES: [&str; 9] = [
    "draft",
    "booked",
    "picked_up",
    "in_transit",
    "customs",
    "out_for_delivery",
    "delivered",
    "cancelled",
    "exception",
];
/// The happy path a shipment walks from booking to delivery.
pub const PIPELINE: [&str; 7] = [
    "draft",
    "booked",
    "picked_up",
    "in_transit",
    "customs",
    "out_for_delivery",
    "delivered",
];
/// States nothing can leave.
pub const TERMINAL: [&str; 2] = ["delivered", "cancelled"];
/// Upper bound for a presigned shipment document.
pub const MAX_DOCUMENT_BYTES: i64 = 50 * 1024 * 1024;

/// Query flags are written `?flag=1` in the API contract; `true`, `yes`, `on` and a
/// bare `?flag` mean the same thing.
pub fn truthy(value: Option<&str>) -> bool {
    matches!(value.map(str::trim), Some("1" | "true" | "yes" | "on" | ""))
}

pub fn check_one_of(field: &'static str, value: &str, allowed: &[&str]) -> ApiResult<()> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(ApiError::validation(
            field,
            format!("must be one of {}", allowed.join(", ")),
        ))
    }
}

/// Money always travels with a three letter code; USD is the house currency.
pub fn currency_or_default(currency: Option<&str>) -> ApiResult<String> {
    match currency {
        None => Ok("USD".to_string()),
        Some(code) if code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) => {
            Ok(code.to_ascii_uppercase())
        }
        Some(_) => Err(ApiError::validation(
            "currency",
            "must be a three letter code",
        )),
    }
}

/// Origins, destinations and addresses are free-form JSON objects, never scalars.
pub fn check_json_object(field: &'static str, value: &serde_json::Value) -> ApiResult<()> {
    if value.is_object() {
        Ok(())
    } else {
        Err(ApiError::validation(field, "must be a JSON object"))
    }
}

pub fn is_terminal(status: &str) -> bool {
    TERMINAL.contains(&status)
}

/// The state that follows `from` on the happy path.
pub fn next_status(from: &str) -> Option<&'static str> {
    let index = PIPELINE.iter().position(|status| *status == from)?;
    PIPELINE.get(index + 1).copied()
}

/// What a legal transition does, once the state machine has accepted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// One step along the pipeline.
    Advance,
    /// Something went wrong; the current state is remembered.
    Exception,
    /// Back to the state the exception interrupted.
    Resume,
    Cancel,
}

/// The state machine from docs/DOMAIN.md:
///
/// ```text
/// draft -> booked -> picked_up -> in_transit -> customs -> out_for_delivery -> delivered
///   any non-terminal state -> exception -> (back to the previous state) | cancelled
///   draft | booked -> cancelled
/// ```
pub fn plan_transition(from: &str, to: &str, previous: Option<&str>) -> ApiResult<Transition> {
    if is_terminal(from) {
        return Err(ApiError::transition(from, to));
    }
    match to {
        "exception" if from != "exception" => Ok(Transition::Exception),
        "cancelled" if matches!(from, "draft" | "booked" | "exception") => Ok(Transition::Cancel),
        _ if from == "exception" => {
            if previous == Some(to) {
                Ok(Transition::Resume)
            } else {
                Err(ApiError::transition(from, to))
            }
        }
        _ if next_status(from) == Some(to) => Ok(Transition::Advance),
        _ => Err(ApiError::transition(from, to)),
    }
}

/// Every state this shipment may legally move to next, so a client can draw one
/// button per option instead of guessing at the state machine.
pub fn allowed_transitions(status: &str, previous: Option<&str>) -> Vec<String> {
    SHIPMENT_STATUSES
        .iter()
        .filter(|to| plan_transition(status, to, previous).is_ok())
        .map(|to| (*to).to_string())
        .collect()
}

/// The timeline entry a transition leaves behind.
pub fn event_for(status: &str, transition: Transition) -> &'static str {
    if transition == Transition::Resume {
        return "resumed";
    }
    match status {
        "booked" => "booked",
        "picked_up" => "picked_up",
        "in_transit" => "departed",
        "customs" => "customs_hold",
        "out_for_delivery" => "out_for_delivery",
        "delivered" => "delivered",
        "cancelled" => "cancelled",
        "exception" => "exception",
        _ => "note",
    }
}

/// The shipment fields every authorisation and state decision needs.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ShipmentCore {
    pub id: Uuid,
    pub reference: String,
    pub status: String,
    pub previous_status: Option<String>,
    pub customer_id: Uuid,
    pub owner_id: Option<Uuid>,
    pub created_by: Option<Uuid>,
}

const SHIPMENT_CORE: &str =
    "select id, reference, status, previous_status, customer_id, owner_id, created_by
       from shipments where id = $1";

pub async fn load_shipment(conn: &mut PgConnection, id: Uuid) -> ApiResult<ShipmentCore> {
    sqlx::query_as(SHIPMENT_CORE)
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("shipment"))
}

/// Same as [`load_shipment`], but holds the row until the transaction ends so two
/// concurrent transitions cannot both read the same starting state.
pub async fn lock_shipment(conn: &mut PgConnection, id: Uuid) -> ApiResult<ShipmentCore> {
    sqlx::query_as(&format!("{SHIPMENT_CORE} for update"))
        .bind(id)
        .fetch_optional(conn)
        .await?
        .ok_or_else(|| ApiError::not_found("shipment"))
}

/// Drivers may report events on the shipments they actually move.
pub async fn is_assigned_driver(
    conn: &mut PgConnection,
    shipment_id: Uuid,
    employee_id: Uuid,
) -> sqlx::Result<bool> {
    sqlx::query_scalar(
        "select exists (select 1 from shipment_legs
                         where shipment_id = $1 and driver_id = $2)",
    )
    .bind(shipment_id)
    .bind(employee_id)
    .fetch_one(conn)
    .await
}

pub fn document_key(shipment_id: Uuid, kind: &str, title: &str) -> String {
    format!(
        "{}{}/{}-{}",
        document_prefix(shipment_id),
        kind,
        Uuid::new_v4(),
        sanitise_filename(title)
    )
}

pub fn document_prefix(shipment_id: Uuid) -> String {
    format!("shipments/{shipment_id}/")
}

/// Everything the analytics service needs to score a shipment.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RiskInputs {
    pub mode: String,
    pub weight_kg: Decimal,
    pub pieces: i32,
    pub hazardous: bool,
    pub etd: Option<NaiveDate>,
    pub eta: Option<NaiveDate>,
    pub carrier_on_time_rate: Option<Decimal>,
}

pub async fn load_risk_inputs(
    conn: &mut PgConnection,
    shipment_id: Uuid,
) -> sqlx::Result<Option<RiskInputs>> {
    sqlx::query_as(
        "select s.mode, s.weight_kg, s.pieces, s.hazardous, s.etd, s.eta,
                (select avg(c.on_time_rate)
                   from shipment_legs l join carriers c on c.id = l.carrier_id
                  where l.shipment_id = s.id) as carrier_on_time_rate
           from shipments s where s.id = $1",
    )
    .bind(shipment_id)
    .fetch_optional(conn)
    .await
}

/// Asks analytics for a 0 to 1 delay-risk score. The client already fails open, so
/// `None` simply means the shipment keeps whatever score it had.
pub async fn delay_risk(analytics: &AnalyticsClient, inputs: &RiskInputs) -> Option<Decimal> {
    analytics
        .delay_risk(&DelayRiskRequest {
            mode: inputs.mode.clone(),
            weight_kg: inputs.weight_kg,
            pieces: inputs.pieces,
            hazardous: inputs.hazardous,
            distance_km: None,
            carrier_on_time_rate: inputs.carrier_on_time_rate,
            etd: inputs.etd,
            eta: inputs.eta,
        })
        .await
}

/// Re-scores a shipment after a change that could move the risk. Scoring is
/// advisory: every failure is logged and swallowed so the caller's write stands.
pub async fn rescore(state: &AppState, shipment_id: Uuid) {
    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(err) => {
            tracing::warn!(error = %err, "delay risk not refreshed: no connection");
            return;
        }
    };
    let inputs = match load_risk_inputs(&mut conn, shipment_id).await {
        Ok(Some(inputs)) => inputs,
        Ok(None) => return,
        Err(err) => {
            tracing::warn!(error = %err, "delay risk not refreshed: shipment unreadable");
            return;
        }
    };
    let Some(risk) = delay_risk(&state.analytics, &inputs).await else {
        return;
    };
    if let Err(err) = sqlx::query("update shipments set delay_risk = $2 where id = $1")
        .bind(shipment_id)
        .bind(risk)
        .execute(&mut *conn)
        .await
    {
        tracing::warn!(error = %err, "could not store the delay risk score");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pipeline_advances_one_step_at_a_time() {
        assert_eq!(
            plan_transition("draft", "booked", None).unwrap(),
            Transition::Advance
        );
        assert_eq!(
            plan_transition("customs", "out_for_delivery", None).unwrap(),
            Transition::Advance
        );
        assert!(plan_transition("draft", "in_transit", None).is_err());
        assert!(plan_transition("in_transit", "booked", None).is_err());
    }

    #[test]
    fn exceptions_remember_where_they_came_from() {
        assert_eq!(
            plan_transition("in_transit", "exception", None).unwrap(),
            Transition::Exception
        );
        assert_eq!(
            plan_transition("exception", "in_transit", Some("in_transit")).unwrap(),
            Transition::Resume
        );
        assert!(plan_transition("exception", "customs", Some("in_transit")).is_err());
        assert_eq!(
            plan_transition("exception", "cancelled", Some("in_transit")).unwrap(),
            Transition::Cancel
        );
    }

    #[test]
    fn only_early_shipments_can_be_cancelled() {
        assert_eq!(
            plan_transition("draft", "cancelled", None).unwrap(),
            Transition::Cancel
        );
        assert_eq!(
            plan_transition("booked", "cancelled", None).unwrap(),
            Transition::Cancel
        );
        assert!(plan_transition("in_transit", "cancelled", None).is_err());
    }

    #[test]
    fn terminal_states_are_final() {
        assert!(plan_transition("delivered", "exception", None).is_err());
        assert!(plan_transition("cancelled", "booked", None).is_err());
    }

    #[test]
    fn the_allowed_set_matches_the_state_machine() {
        assert_eq!(
            allowed_transitions("draft", None),
            vec!["booked", "cancelled", "exception"]
        );
        assert_eq!(
            allowed_transitions("in_transit", None),
            vec!["customs", "exception"]
        );
        assert_eq!(
            allowed_transitions("exception", Some("in_transit")),
            vec!["in_transit", "cancelled"]
        );
        assert!(allowed_transitions("delivered", None).is_empty());
        assert!(allowed_transitions("cancelled", None).is_empty());
    }

    #[test]
    fn transitions_name_their_timeline_entry() {
        assert_eq!(event_for("in_transit", Transition::Advance), "departed");
        assert_eq!(event_for("customs", Transition::Resume), "resumed");
        assert_eq!(event_for("exception", Transition::Exception), "exception");
    }
}
