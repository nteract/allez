//! Channel-configuration observability types.

/// Version of the structured channel-configuration fallback event schema.
pub(crate) const CHANNEL_CONFIG_EVENT_SCHEMA_VERSION: &str = "1";

/// Distinguishes FR-011's two recorded fallback cases — shared between
/// `ChannelConfigFallbackEvent` (the observability record) and
/// `ChannelConfigResolution::Ready`'s `fallback` field (the same fact,
/// made inspectable in the return value itself, FR-017/research.md R10).
/// `#[non_exhaustive]` for the same future-proofing reason every other
/// public enum in this ticket's scope already uses.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// A `~/.condarc` `parse()` rejected, or whose `expand_channels()`
    /// call failed (FR-011/FR-018, the latter broadened into this same
    /// variant per research.md R11).
    Rejected,
    /// A `~/.condarc` that exists but could not be read due to an OS
    /// permission or other I/O error (FR-011).
    Unreadable,
}

struct ChannelConfigFallbackEvent {
    schema_version: &'static str,
    reason: FallbackReason,
    detail: String,
}

pub(crate) fn emit_fallback(reason: FallbackReason, detail: &str) {
    let event = ChannelConfigFallbackEvent {
        schema_version: CHANNEL_CONFIG_EVENT_SCHEMA_VERSION,
        reason,
        detail: detail.to_string(),
    };
    tracing::warn!(
        schema_version = event.schema_version,
        reason = ?event.reason,
        detail = event.detail.as_str(),
    );
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tracing::{Event, Subscriber, field::Visit};
    use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};

    use super::{CHANNEL_CONFIG_EVENT_SCHEMA_VERSION, FallbackReason, emit_fallback};

    type EventFields = Vec<(String, String)>;
    type CapturedEventBuffer = Arc<Mutex<Vec<EventFields>>>;

    #[derive(Clone)]
    struct CapturedEvents(CapturedEventBuffer);

    impl<S> Layer<S> for CapturedEvents
    where
        S: Subscriber + for<'span> LookupSpan<'span>,
    {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut visitor = FieldVisitor(Vec::new());
            event.record(&mut visitor);
            self.0.lock().unwrap().push(visitor.0);
        }
    }

    struct FieldVisitor(EventFields);

    impl Visit for FieldVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0
                .push((field.name().to_string(), format!("{value:?}")));
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.0.push((field.name().to_string(), value.to_string()));
        }
    }

    #[test]
    fn emit_fallback_carries_exact_schema_reason_and_detail_fields() {
        // Given
        let captured = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CapturedEvents(Arc::clone(&captured)));

        // When
        tracing::subscriber::with_default(subscriber, || {
            emit_fallback(FallbackReason::Rejected, "invalid channel setting");
        });

        // Then
        let events = captured.lock().unwrap();
        assert_eq!(events.len(), 1);
        let mut fields = events[0].clone();
        fields.sort();
        assert_eq!(
            fields,
            vec![
                ("detail".to_string(), "invalid channel setting".to_string()),
                ("reason".to_string(), "Rejected".to_string()),
                (
                    "schema_version".to_string(),
                    CHANNEL_CONFIG_EVENT_SCHEMA_VERSION.to_string(),
                ),
            ]
        );
    }
}
