//! Support module for `tests/condarc_conformance.rs` (GEN-36 research R10). A file under
//! `crates/condarc/tests/` is unreachable from this harness's own test target, so the
//! conformance-only adapter lives here instead, as `tests/support/adapter.rs`.
//!
//! This directory's other files (`creation.rs`, `failures.rs`, `defaults.rs`,
//! `ephemeral.rs`) belong to a separate test binary and are wired in
//! directly via `#[path]` in `tests/ephemeral_env.rs`, not through this module.

pub mod adapter;
