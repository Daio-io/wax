//! Shared typed errors for registry lookup operations.
//!
//! This module is the core export boundary for lookup failures. Cache, context,
//! and search implementations convert into [`RegistryLookupError`] without
//! exposing their internal error types across crate borders.

pub use wax_contract::{RegistryLookupError, RegistryLookupErrorCode};
