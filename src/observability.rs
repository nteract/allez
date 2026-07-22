//! Structured observability: `tracing-subscriber` initialization selecting
//! a human or JSON formatter based on the parsed `--human` flag (FR-013,
//! constitution Principle XI). All `tracing::*!` events go to stderr,
//! separate from the primary result payload on stdout (research.md §3).

use tracing_subscriber::EnvFilter;

/// Initializes the global `tracing` subscriber. Must be called once, before
/// argv is even parsed, since the formatter choice cannot be changed after
/// `.try_init()` runs and RUST_LOG must also apply to parse-time usage
/// errors (Constitution XI). Selects the `.pretty()` formatter when `human`
/// is set, `.json()` (the default) otherwise; both write to stderr.
///
/// Emits nothing unless `RUST_LOG` is explicitly set: the fallback filter
/// is `"off"`, not a nonzero default level. Structured events are additive
/// diagnostics, layered onto the same stderr stream `render_error`'s fixed
/// `{schema_version, category, message}` error body already owns by
/// default (FR-010, FR-011) — a nonzero default level would interleave log
/// lines with that body and break every caller (human or agent) parsing
/// stderr as exactly one JSON object on failure. `RUST_LOG=info` (or
/// `debug`/`trace`) opts in explicitly, standard Rust-CLI convention.
pub fn init(human: bool) {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("off"));
    let builder = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(env_filter);
    let result = if human {
        builder.pretty().try_init()
    } else {
        builder.json().try_init()
    };
    if let Err(err) = result {
        eprintln!("warning: failed to initialize tracing subscriber: {err}");
    }
}
