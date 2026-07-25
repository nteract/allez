//! The shared coercion engine, one module per `ValueKind` family. Each module implements both
//! the accept-path (User Story 1) and reject-path (User Story 2) of the same coercion functions.

pub mod boolish;
pub mod enums;
pub mod numeric;
pub mod sequences;
pub mod strings;
