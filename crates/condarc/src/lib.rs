//! `condarc` — a standalone, publishable Rust library that parses the text of a `.condarc`
//! YAML document into a validated, strongly-typed `Config`, or into a `ValidationReport` that
//! accumulates every independent problem found in the document.
//!
//! This crate never opens a file itself; callers pass the already-read `.condarc` text in as a
//! `&str`.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

mod catalog;
mod coerce;
mod error;
mod model;
mod parse;
mod validate;

pub use error::{ErrorEntry, ErrorKind, InputRepr, Location, PathSegment, ValidationReport};
pub use model::{
    BoolOrInt, ChannelPriority, ChannelSetting, Config, ListField, ParseOptions, PathConflict,
    SafetyChecks, SatSolver, SslVerify,
};

/// Parse the text of a single `.condarc` document into a typed [`Config`], or a
/// [`ValidationReport`] accumulating every problem found. Equivalent to
/// `parse_with_options(yaml, ParseOptions::default())` — the hermetic, conformance-portable path
/// (no filesystem access).
///
/// The caller is responsible for locating and reading the file (FR-001). Never panics on any
/// input (FR-004).
///
/// # Errors
/// Returns `Err(ValidationReport)` on YAML syntax errors, non-mapping roots, or any accumulated
/// per-setting / alias / cross-field validation failure.
pub fn parse(yaml: &str) -> Result<Config, ValidationReport> {
    parse_with_options(yaml, ParseOptions::default())
}

/// Parse with explicit [`ParseOptions`]. Use this to opt into the real filesystem `ssl_verify`
/// existence check (`ssl_verify_fs_check: true`) for exact conda runtime fidelity; the default
/// [`parse`] never touches the filesystem and accepts an `ssl_verify` path string unverified
/// (FR-002/FR-024, research R6).
///
/// # Errors
/// Same failure modes as [`parse`].
pub fn parse_with_options(yaml: &str, options: ParseOptions) -> Result<Config, ValidationReport> {
    parse::parse_document(yaml, &options)
}
