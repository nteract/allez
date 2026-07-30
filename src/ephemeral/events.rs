//! `EphemeralLifecycleEvent` and the `tracing`-emission/redaction helper.

use std::fmt;

use super::{channels::redact_channel_url, lifecycle::EnvironmentId};

/// Version of the structured ephemeral lifecycle event schema.
pub const EPHEMERAL_EVENT_SCHEMA_VERSION: &str = "1";

/// Structured observability data for one ephemeral environment lifecycle step.
#[derive(Clone)]
pub struct EphemeralLifecycleEvent {
    /// Version of this event schema.
    pub schema_version: &'static str,
    /// Environment whose lifecycle produced this event.
    pub environment_id: EnvironmentId,
    /// Lifecycle operation: `create`, `install`, or `teardown`.
    pub operation: &'static str,
    /// Effective top-level package specifications.
    pub packages: Vec<String>,
    /// Operation duration in milliseconds.
    pub duration_ms: u64,
    /// Operation outcome: `success` or `failure`.
    pub outcome: &'static str,
    /// Fixed failure category when the outcome is `failure`.
    pub failure_category: Option<&'static str>,
}

impl fmt::Debug for EphemeralLifecycleEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let packages = self
            .packages
            .iter()
            .map(|package| redact_channel_url(package))
            .collect::<Vec<_>>();
        formatter
            .debug_struct("EphemeralLifecycleEvent")
            .field("schema_version", &self.schema_version)
            .field("environment_id", &self.environment_id)
            .field("operation", &self.operation)
            .field("packages", &packages)
            .field("duration_ms", &self.duration_ms)
            .field("outcome", &self.outcome)
            .field("failure_category", &self.failure_category)
            .finish()
    }
}

/// Emits one credential-redacted structured lifecycle event through `tracing`.
pub fn emit_event(event: &EphemeralLifecycleEvent) {
    let packages = event
        .packages
        .iter()
        .map(|package| redact_channel_url(package))
        .collect::<Vec<_>>();
    match event.outcome {
        "success" => tracing::info!(
            schema_version = event.schema_version,
            environment_id = %event.environment_id,
            operation = event.operation,
            packages = ?packages,
            duration_ms = event.duration_ms,
            outcome = event.outcome,
            failure_category = ?event.failure_category,
            "ephemeral environment lifecycle"
        ),
        "failure" => tracing::error!(
            schema_version = event.schema_version,
            environment_id = %event.environment_id,
            operation = event.operation,
            packages = ?packages,
            duration_ms = event.duration_ms,
            outcome = event.outcome,
            failure_category = ?event.failure_category,
            "ephemeral environment lifecycle"
        ),
        _ => tracing::error!(
            schema_version = event.schema_version,
            environment_id = %event.environment_id,
            operation = event.operation,
            packages = ?packages,
            duration_ms = event.duration_ms,
            outcome = event.outcome,
            failure_category = ?event.failure_category,
            "invalid ephemeral environment lifecycle outcome"
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::{Event, Subscriber, field::Visit};
    use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};

    use crate::ephemeral::lifecycle::EnvironmentId;

    use super::{EPHEMERAL_EVENT_SCHEMA_VERSION, EphemeralLifecycleEvent, emit_event};

    #[derive(Clone)]
    struct CapturedFields(Arc<Mutex<Vec<String>>>);

    impl<S> Layer<S> for CapturedFields
    where
        S: Subscriber + for<'span> LookupSpan<'span>,
    {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut visitor = FieldVisitor(Vec::new());
            event.record(&mut visitor);
            self.0.lock().unwrap().extend(visitor.0);
        }
    }

    struct FieldVisitor(Vec<String>);

    impl Visit for FieldVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0.push(format!("{}={value:?}", field.name()));
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.0.push(format!("{}={value}", field.name()));
        }

        fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
            self.0.push(format!("{}={value}", field.name()));
        }
    }

    #[test]
    fn emitted_event_carries_schema_version_and_stable_environment_id() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CapturedFields(Arc::clone(&captured)));
        let environment_id = EnvironmentId::new();
        let expected_id = environment_id.to_string();
        let event = EphemeralLifecycleEvent {
            schema_version: EPHEMERAL_EVENT_SCHEMA_VERSION,
            environment_id,
            operation: "create",
            packages: vec!["python".to_string()],
            duration_ms: 12,
            outcome: "success",
            failure_category: None,
        };

        tracing::subscriber::with_default(subscriber, || emit_event(&event));

        let fields = captured.lock().unwrap().join(" ");
        assert!(fields.contains(EPHEMERAL_EVENT_SCHEMA_VERSION));
        assert!(fields.contains(&expected_id));
    }

    #[test]
    fn emitted_event_never_contains_channel_credentials() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CapturedFields(Arc::clone(&captured)));
        let event = EphemeralLifecycleEvent {
            schema_version: EPHEMERAL_EVENT_SCHEMA_VERSION,
            environment_id: EnvironmentId::new(),
            operation: "install",
            packages: vec![
                "https://user:password@repo.example/t/token-123/conda-forge".to_string(),
            ],
            duration_ms: 12,
            outcome: "failure",
            failure_category: Some("unresolvable_package"),
        };

        tracing::subscriber::with_default(subscriber, || emit_event(&event));

        let fields = captured.lock().unwrap().join(" ");
        assert!(!fields.contains("user:password"));
        assert!(!fields.contains("token-123"));
        assert!(fields.contains("https://repo.example/conda-forge"));
    }

    #[test]
    fn lifecycle_event_debug_never_contains_package_credentials() {
        // Given
        let event = EphemeralLifecycleEvent {
            schema_version: EPHEMERAL_EVENT_SCHEMA_VERSION,
            environment_id: EnvironmentId::new(),
            operation: "install",
            packages: vec![
                "https://user:password@repo.example/t/token-123/conda-forge::numpy".to_string(),
            ],
            duration_ms: 12,
            outcome: "failure",
            failure_category: Some("unresolvable_package"),
        };

        // When
        let message = format!("{event:?}");

        // Then
        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(message.contains("https://repo.example/conda-forge::numpy"));
    }
}
