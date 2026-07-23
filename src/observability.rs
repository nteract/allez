//! Structured observability: all `tracing::*!` events go to stderr,
//! separate from the primary result payload on stdout.

use tracing_subscriber::EnvFilter;

/// Must be called once, before argv is even parsed, since the formatter
/// choice cannot be changed after `.try_init()` runs and `RUST_LOG` must
/// also apply to parse-time usage errors. Selects the `.pretty()`
/// formatter when `human` is set, `.json()` (the default) otherwise; both
/// write to stderr.
///
/// Emits nothing unless `RUST_LOG` is explicitly set: the fallback filter
/// is `"off"`, not a nonzero default level — a nonzero default would
/// interleave log lines with `render_error`'s single-JSON-object error
/// body on stderr and break every caller parsing stderr as exactly one
/// JSON object on failure.
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
