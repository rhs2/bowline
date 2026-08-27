//! Bowline core service: identity, organisation, HR, operations, finance,
//! communications, support desk and audit trail.

pub mod admin;
pub mod audit;
pub mod auth;
pub mod clients;
pub mod comms;
pub mod config;
pub mod dashboard;
pub mod db;
pub mod error;
pub mod finance;
pub mod health;
pub mod hr;
pub mod http;
pub mod openapi;
pub mod ops;
pub mod org;
pub mod outbox;
pub mod scope;
pub mod state;
pub mod support;
pub mod telemetry;

pub use config::Config;
pub use state::AppState;
