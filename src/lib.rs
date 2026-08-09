#![warn(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

//! `allez` library: the CLI scaffold plus the ephemeral-environment core
//! (GEN-24). `src/main.rs` is a thin binary shim over this library target.

/// `.condarc` file handling and channel-configuration resolution.
pub mod channel_config;
/// The CLI scaffold (argument parsing, subcommand stubs) — unmodified by
/// GEN-24; wiring the ephemeral-environment core into a real subcommand
/// is GEN-25's job.
pub mod cli;
/// Resolution of the ephemeral-environment default package set from an
/// already-resolved `.condarc` document (GEN-30).
mod default_packages_config;
/// The ephemeral-environment core: create and populate an unnamed,
/// caller-unpathed conda environment (GEN-24).
pub mod ephemeral;
/// The fixed usage-error categories (`AllezError`) plus the shared
/// [`CategorizedError`](error::CategorizedError) trait every
/// fixed-category error type in this crate implements.
pub mod error;
/// `tracing`/`tracing-subscriber` initialization — unmodified by GEN-24.
pub mod observability;
/// JSON/human error and success rendering — unmodified by GEN-24.
pub mod output;
