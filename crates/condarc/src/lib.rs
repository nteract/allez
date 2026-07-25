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
