//! Dolphin Pod core. Everything the apps need (storage, providers, routing, permissions, usage,
//! scheduling) lives here with no UI-framework dependency, so the desktop app links it directly
//! and the iOS app can consume it through UniFFI in Phase 2.

pub mod attachments;
pub mod catalog;
pub mod core;
pub mod db;
mod engine;
pub mod error;
pub mod events;
pub mod model;
pub mod permissions;
pub mod preferences;
pub mod providers;
pub mod router;
pub mod scheduler;
pub mod secrets;
pub mod seed;
pub mod settings;
pub mod tools;
pub mod update;
pub mod util;

pub use crate::core::{Core, CoreRef};
pub use error::{CoreError, Result};
