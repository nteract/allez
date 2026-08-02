//! `.condarc` file handling and channel-configuration resolution.

use std::path::{Path, PathBuf};

use condarc::Config;

mod events;
mod locate;

use events::emit_fallback;
use locate::{ReadOutcome, read_condarc};

pub use events::FallbackReason;

/// The result of one channel-configuration resolution call. A
/// `#[non_exhaustive]` enum, not a struct (research.md R13): the ordinary
/// case, `Ready`, pairs the crate's own `ResolvedChannels`
/// (FR-012/FR-016, research.md R15 — now GEN-24's own required
/// channel-configuration input directly) with a sibling signal
/// distinguishing a rejected/unreadable/unexpandable-file fallback
/// (FR-011/FR-018) from the silent missing-file case (FR-009) — see
/// `FallbackReason`, defined alongside the observability types that
/// already model this distinction (research.md R10). The other case,
/// `NoChannels`, exists so a resolution that legitimately produced zero
/// usable channels (FR-019's filtering removed every entry, or the
/// underlying `.condarc` configuration otherwise resolves to an empty list
/// — spec.md Design Decisions, "Empty resolved list, two legitimate
/// causes") is never mistaken for "nothing was configured" — see FR-020
/// and research.md R13/R15 for why this distinction still matters even
/// after GEN-24's own `channels_with_fallback` is retired.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelConfigResolution {
    /// A resolution that produced at least one usable channel, whether
    /// from a real, populated `~/.condarc` or from conda's own
    /// documented defaults (missing/rejected/unreadable/unexpandable
    /// fallback).
    Ready {
        /// The crate's own `ResolvedChannels`, exactly as
        /// `condarc::expand_channels()` produced it — never itself
        /// carries the fallback signal (FR-017 keeps that a sibling
        /// field, not a third field on this type). This is GEN-24's own
        /// required channel-configuration input directly (FR-012/FR-016,
        /// research.md R15) — no adaptation, no intermediate type.
        config: condarc::ResolvedChannels,
        /// `None` for a fully-successful resolution and for the silent
        /// missing-file case (FR-009). `Some(FallbackReason::Rejected)`
        /// or `Some(FallbackReason::Unreadable)` for FR-011's recorded
        /// fallback cases (`Rejected` broadened to also cover an
        /// `ExpandChannelsError`, FR-018/research.md R11) — the same
        /// cases `ChannelConfigFallbackEvent` already records via
        /// observability; this field makes that same fact inspectable
        /// in the return value itself (FR-017).
        fallback: Option<FallbackReason>,
    },
    /// The resolved configuration legitimately has zero usable channels
    /// (FR-019's allow/deny filtering removed every entry, or the
    /// underlying `.condarc` configuration otherwise resolves to an
    /// empty list) — a fully successful resolution of the user's own real
    /// preferences, not a fallback and not an error (FR-020). Carries no
    /// payload: there is no channel configuration for a caller to
    /// mistakenly pass to GEN-24.
    NoChannels,
}

fn default_condarc_path() -> Option<PathBuf> {
    dirs::home_dir().map(|home| home.join(".condarc"))
}

#[allow(clippy::expect_used)]
fn default_resolved_channels() -> condarc::ResolvedChannels {
    condarc::expand_channels(&Config::default()).expect(
        "expand_channels(&Config::default()) is documented to never fail; if it does, the crate's own built-in defaults changed incompatibly",
    )
}

fn into_resolution(
    resolved: condarc::ResolvedChannels,
    fallback: Option<FallbackReason>,
) -> ChannelConfigResolution {
    if resolved.channels.is_empty() {
        ChannelConfigResolution::NoChannels
    } else {
        ChannelConfigResolution::Ready {
            config: resolved,
            fallback,
        }
    }
}

pub(crate) fn resolve_channel_config_from(path: Option<&Path>) -> ChannelConfigResolution {
    let Some(path) = path else {
        return into_resolution(default_resolved_channels(), None);
    };

    let contents = match read_condarc(path) {
        Ok(contents) => contents,
        Err(ReadOutcome::Missing) => return into_resolution(default_resolved_channels(), None),
        Err(ReadOutcome::Unreadable(error)) => {
            emit_fallback(FallbackReason::Unreadable, &error.to_string());
            return into_resolution(
                default_resolved_channels(),
                Some(FallbackReason::Unreadable),
            );
        }
    };

    let config = match condarc::parse(&contents) {
        Ok(config) => config,
        Err(report) => {
            emit_fallback(FallbackReason::Rejected, &report.to_string());
            return into_resolution(default_resolved_channels(), Some(FallbackReason::Rejected));
        }
    };

    match condarc::expand_channels(&config) {
        Ok(resolved) => into_resolution(resolved, None),
        Err(error) => {
            emit_fallback(FallbackReason::Rejected, &error.to_string());
            into_resolution(default_resolved_channels(), Some(FallbackReason::Rejected))
        }
    }
}

/// Resolves the current user's `.condarc`, falling back to conda's defaults.
pub fn resolve_channel_config() -> ChannelConfigResolution {
    resolve_channel_config_from(default_condarc_path().as_deref())
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        io::Write,
        sync::{Arc, Mutex},
    };

    use tempfile::NamedTempFile;
    use tracing::{Event, Subscriber, field::Visit};
    use tracing_subscriber::{Layer, layer::Context, prelude::*, registry::LookupSpan};

    use super::{ChannelConfigResolution, FallbackReason, resolve_channel_config_from};

    const PARSE_REJECTED: &str = "channels: [alpha]\nchannel: [beta]\n";
    const EXPANSION_REJECTED: &str = "channels: [alpha]\nchannel_alias: \"\"\n";

    #[derive(Clone)]
    struct CapturedEvents(Arc<Mutex<Vec<BTreeMap<String, String>>>>);

    impl<S> Layer<S> for CapturedEvents
    where
        S: Subscriber + for<'span> LookupSpan<'span>,
    {
        fn on_event(&self, event: &Event<'_>, _context: Context<'_, S>) {
            let mut visitor = FieldVisitor(BTreeMap::new());
            event.record(&mut visitor);
            self.0.lock().unwrap().push(visitor.0);
        }
    }

    struct FieldVisitor(BTreeMap<String, String>);

    impl Visit for FieldVisitor {
        fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
            self.0
                .insert(field.name().to_string(), format!("{value:?}"));
        }

        fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
            self.0.insert(field.name().to_string(), value.to_string());
        }
    }

    fn write_temp_file(contents: &[u8]) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(contents).unwrap();
        file
    }

    fn capture_events(action: impl FnOnce()) -> Vec<BTreeMap<String, String>> {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let subscriber = tracing_subscriber::registry().with(CapturedEvents(Arc::clone(&captured)));

        tracing::subscriber::with_default(subscriber, action);

        Arc::try_unwrap(captured).unwrap().into_inner().unwrap()
    }

    fn default_resolved_channels() -> condarc::ResolvedChannels {
        condarc::expand_channels(&condarc::Config::default())
            .expect("Config::default must resolve in tests")
    }

    fn expect_ready(
        resolution: ChannelConfigResolution,
    ) -> (condarc::ResolvedChannels, Option<FallbackReason>) {
        match resolution {
            ChannelConfigResolution::Ready { config, fallback } => (config, fallback),
            ChannelConfigResolution::NoChannels => panic!("expected ready channel configuration"),
        }
    }

    #[test]
    fn parse_rejection_emits_one_rejected_fallback_event() {
        // Given
        let file = write_temp_file(PARSE_REJECTED.as_bytes());

        // When
        let events = capture_events(|| {
            let _ = resolve_channel_config_from(Some(file.path()));
        });

        // Then
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].get("reason").map(String::as_str),
            Some("Rejected")
        );
    }

    #[test]
    fn invalid_utf8_emits_one_unreadable_fallback_event() {
        // Given
        let file = write_temp_file(&[0xff, 0xfe, 0xfd]);

        // When
        let events = capture_events(|| {
            let _ = resolve_channel_config_from(Some(file.path()));
        });

        // Then
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].get("reason").map(String::as_str),
            Some("Unreadable")
        );
    }

    #[test]
    fn expansion_failure_emits_one_rejected_fallback_event_with_error_detail() {
        // Given
        let file = write_temp_file(EXPANSION_REJECTED.as_bytes());
        let config = condarc::parse(EXPANSION_REJECTED).expect("fixture must parse");
        let expected_detail = condarc::expand_channels(&config)
            .expect_err("fixture must fail expansion")
            .to_string();

        // When
        let events = capture_events(|| {
            let _ = resolve_channel_config_from(Some(file.path()));
        });

        // Then
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].get("reason").map(String::as_str),
            Some("Rejected")
        );
        assert_eq!(
            events[0].get("detail").map(String::as_str),
            Some(expected_detail.as_str())
        );
    }

    #[test]
    fn missing_file_emits_no_fallback_event() {
        // Given
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = temporary_directory.path().join("missing.condarc");

        // When
        let events = capture_events(|| {
            let _ = resolve_channel_config_from(Some(&path));
        });

        // Then
        assert!(events.is_empty());
    }

    #[test]
    fn nonexistent_path_returns_ready_defaults_without_fallback_reason() {
        // Given
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = temporary_directory.path().join("missing.condarc");
        let expected = default_resolved_channels();

        // When
        let resolution = resolve_channel_config_from(Some(&path));

        // Then
        assert_eq!(expect_ready(resolution), (expected, None));
    }

    #[test]
    fn parse_rejection_returns_ready_defaults_with_rejected_reason() {
        // Given
        let file = write_temp_file(PARSE_REJECTED.as_bytes());
        let expected = default_resolved_channels();

        // When
        let resolution = resolve_channel_config_from(Some(file.path()));

        // Then
        assert_eq!(
            expect_ready(resolution),
            (expected, Some(FallbackReason::Rejected))
        );
    }

    #[test]
    fn invalid_utf8_returns_ready_defaults_with_unreadable_reason() {
        // Given
        let file = write_temp_file(&[0xff, 0xfe, 0xfd]);
        let expected = default_resolved_channels();

        // When
        let resolution = resolve_channel_config_from(Some(file.path()));

        // Then
        assert_eq!(
            expect_ready(resolution),
            (expected, Some(FallbackReason::Unreadable))
        );
    }

    #[test]
    fn expansion_failure_returns_ready_defaults_with_rejected_reason() {
        // Given
        let file = write_temp_file(EXPANSION_REJECTED.as_bytes());
        let expected = default_resolved_channels();

        // When
        let resolution = resolve_channel_config_from(Some(file.path()));

        // Then
        assert_eq!(
            expect_ready(resolution),
            (expected, Some(FallbackReason::Rejected))
        );
    }

    #[test]
    fn populated_file_returns_its_resolved_channels_without_fallback_reason() {
        // Given
        let contents = "channels: [alpha, https://repo.example.org/beta]\n";
        let file = write_temp_file(contents.as_bytes());
        let config = condarc::parse(contents).expect("fixture must parse");
        let expected = condarc::expand_channels(&config).expect("fixture must resolve");

        // When
        let resolution = resolve_channel_config_from(Some(file.path()));

        // Then
        assert_eq!(expect_ready(resolution), (expected, None));
    }

    #[test]
    fn absent_path_argument_returns_ready_defaults_without_fallback_reason() {
        // Given
        let expected = default_resolved_channels();

        // When
        let resolution = resolve_channel_config_from(None);

        // Then
        assert_eq!(expect_ready(resolution), (expected, None));
    }

    #[test]
    fn real_world_samples_pass_through_expanded_config_unchanged() {
        // Given
        let samples = [
            "channels: [conda-forge, defaults]\n",
            r#"channels: [acme/label/dev, team]
custom_channels: {acme: "https://internal.example.com"}
custom_multichannels: {team: ["https://other.example.com/x", member2]}
"#,
            "channels: [defaults, https://custom.example.com/chan]\n",
            "channels: [conda-forge]\nchannel_priority: true\n",
            "{}\n",
            r#"channels: [alpha, beta, gamma]
channel_alias: https://example.com
allowlist_channels: [alpha, beta]
denylist_channels: [beta]
"#,
        ];

        for (sample_index, contents) in samples.iter().enumerate() {
            let file = write_temp_file(contents.as_bytes());
            let config = condarc::parse(contents).expect("sample must parse");
            let expected = condarc::expand_channels(&config).expect("sample must resolve");

            // When
            let resolution = resolve_channel_config_from(Some(file.path()));

            // Then
            assert_eq!(
                expect_ready(resolution),
                (expected, None),
                "sample {sample_index}"
            );
        }
    }

    #[test]
    fn empty_defaults_multichannel_returns_no_channels() {
        // Given
        let file = write_temp_file(b"custom_multichannels: {defaults: []}\n");

        // When
        let resolution = resolve_channel_config_from(Some(file.path()));

        // Then
        assert_eq!(resolution, ChannelConfigResolution::NoChannels);
    }

    #[test]
    fn filtering_nonempty_channels_to_empty_returns_no_channels() {
        // Given
        let file = write_temp_file(
            b"channels: [alpha, beta]\nallowlist_channels: [alpha]\ndenylist_channels: [alpha]\n",
        );

        // When
        let resolution = resolve_channel_config_from(Some(file.path()));

        // Then
        assert_eq!(resolution, ChannelConfigResolution::NoChannels);
    }

    #[test]
    fn resolution_never_changes_existing_file_contents_or_mtime() {
        // Given
        let states: [&[u8]; 4] = [
            b"channels: [conda-forge]\n",
            PARSE_REJECTED.as_bytes(),
            &[0xff, 0xfe, 0xfd],
            EXPANSION_REJECTED.as_bytes(),
        ];

        for contents in states {
            let file = write_temp_file(contents);
            let before_contents = std::fs::read(file.path()).unwrap();
            let before_modified = std::fs::metadata(file.path()).unwrap().modified().unwrap();

            // When
            let _ = resolve_channel_config_from(Some(file.path()));

            // Then
            assert_eq!(std::fs::read(file.path()).unwrap(), before_contents);
            assert_eq!(
                std::fs::metadata(file.path()).unwrap().modified().unwrap(),
                before_modified
            );
        }
    }

    #[test]
    fn unrecognized_top_level_key_resolves_exactly_as_if_absent() {
        // Given
        let baseline = write_temp_file(b"channels: [alpha]\n");
        let with_unknown =
            write_temp_file(b"channels: [alpha]\nunrelated_future_setting: arbitrary\n");

        // When
        let baseline_resolution = resolve_channel_config_from(Some(baseline.path()));
        let unknown_resolution = resolve_channel_config_from(Some(with_unknown.path()));

        // Then
        assert_eq!(unknown_resolution, baseline_resolution);
    }

    #[test]
    fn consecutive_calls_read_changed_contents_without_caching() {
        // Given
        let file = write_temp_file(b"channels: [alpha]\n");

        // When
        let first = resolve_channel_config_from(Some(file.path()));
        std::fs::write(file.path(), b"channels: [beta]\n").unwrap();
        let second = resolve_channel_config_from(Some(file.path()));

        // Then
        assert_ne!(first, second);
        assert_eq!(
            expect_ready(first).0.channels,
            vec!["https://conda.anaconda.org/alpha".to_string()]
        );
        assert_eq!(
            expect_ready(second).0.channels,
            vec!["https://conda.anaconda.org/beta".to_string()]
        );
    }
}
