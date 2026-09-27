// Modified by Atlas from upstream OpenAI Codex (Apache-2.0). See CONTEXT.md.
//! Cloud-hosted configuration data for Atlas Agent.
//!
//! This crate owns transport, caching, and refresh behavior for cloud-delivered
//! config data. Parsing and composition remain in `atlas-engine-config`.

mod backend;
mod bundle_loader;
mod cache;
mod metrics;
mod service;
mod validation;

pub use bundle_loader::cloud_config_bundle_loader;
pub use bundle_loader::cloud_config_bundle_loader_for_storage;
