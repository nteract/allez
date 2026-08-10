//! The shared `.condarc` location/read/parse step (GEN-30 research.md's
//! first Decision): one file read per invocation, handed to two
//! independent consumers — [`super::channels_from_document`] (channel
//! semantics) and `crate::default_packages_config` (`create_default_packages`
//! extraction) — instead of each resolving, reading, and falling back on
//! its own copy of this logic.

use std::path::Path;

use super::default_condarc_path;
use super::events::{FallbackReason, emit_fallback};
use super::locate::{ReadOutcome, read_condarc};

/// The result of locating, reading, and parsing `~/.condarc` exactly once.
/// A sum type, not a tuple: the three cases are mutually exclusive by
/// construction, so no invalid combination is representable.
pub(crate) enum CondarcDocument {
    /// No file exists at the resolved path (GEN-23's silent missing-file
    /// case) — every caller treats this identically to a file that exists
    /// but configures nothing.
    Absent,
    /// The file exists but could not be read, or `condarc::parse()`
    /// rejected it. [`emit_fallback`] has already fired exactly once
    /// before this variant is returned.
    FellBack(FallbackReason),
    /// A successfully parsed document, so two consumers can read
    /// different fields of the same parse result without a second read.
    /// Boxed: an unboxed `Config` makes this variant dwarf the other two
    /// (`clippy::large_enum_variant`).
    Parsed(Box<condarc::Config>),
}

/// Locates, reads, and parses the current user's `~/.condarc`.
pub(crate) fn resolve_document() -> CondarcDocument {
    resolve_document_from(default_condarc_path().as_deref())
}

/// Test-seam variant of [`resolve_document`] taking an explicit path.
/// `path == None` means "no path was supplied for this call" and resolves
/// directly to [`CondarcDocument::Absent`]; it never re-resolves the
/// default `~/.condarc` location.
pub(crate) fn resolve_document_from(path: Option<&Path>) -> CondarcDocument {
    let Some(path) = path else {
        return CondarcDocument::Absent;
    };

    let contents = match read_condarc(path) {
        Ok(contents) => contents,
        Err(ReadOutcome::Missing) => return CondarcDocument::Absent,
        Err(ReadOutcome::Unreadable(error)) => {
            emit_fallback(FallbackReason::Unreadable, &error.to_string());
            return CondarcDocument::FellBack(FallbackReason::Unreadable);
        }
    };

    match condarc::parse(&contents) {
        Ok(config) => CondarcDocument::Parsed(Box::new(config)),
        Err(report) => {
            emit_fallback(FallbackReason::Rejected, &report.to_string());
            CondarcDocument::FellBack(FallbackReason::Rejected)
        }
    }
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

    use super::{CondarcDocument, FallbackReason, resolve_document_from};

    const PARSE_REJECTED: &str = "channels: [alpha]\nchannel: [beta]\n";

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

        captured.lock().unwrap().clone()
    }

    fn expect_parsed(document: CondarcDocument) -> condarc::Config {
        match document {
            CondarcDocument::Parsed(config) => *config,
            CondarcDocument::Absent => panic!("expected a parsed document, found Absent"),
            CondarcDocument::FellBack(reason) => {
                panic!("expected a parsed document, found FellBack({reason:?})")
            }
        }
    }

    #[test]
    fn resolve_document_from_missing_path_resolves_to_absent() {
        // Given
        let temporary_directory = tempfile::tempdir().unwrap();
        let path = temporary_directory.path().join("missing.condarc");

        // When
        let events = capture_events(|| {
            let document = resolve_document_from(Some(&path));

            // Then
            assert!(matches!(document, CondarcDocument::Absent));
        });

        // Then
        assert!(events.is_empty());
    }

    #[test]
    fn resolve_document_from_absent_path_argument_resolves_to_absent() {
        // Given/When
        let document = resolve_document_from(None);

        // Then
        assert!(matches!(document, CondarcDocument::Absent));
    }

    #[test]
    fn resolve_document_from_unreadable_file_falls_back_and_emits_event() {
        // Given
        let file = write_temp_file(&[0xff, 0xfe, 0xfd]);

        // When
        let events = capture_events(|| {
            let document = resolve_document_from(Some(file.path()));

            // Then
            assert!(matches!(
                document,
                CondarcDocument::FellBack(FallbackReason::Unreadable)
            ));
        });

        // Then
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].get("reason").map(String::as_str),
            Some("Unreadable")
        );
    }

    #[test]
    fn resolve_document_from_malformed_yaml_falls_back_and_emits_event() {
        // Given
        let file = write_temp_file(PARSE_REJECTED.as_bytes());

        // When
        let events = capture_events(|| {
            let document = resolve_document_from(Some(file.path()));

            // Then
            assert!(matches!(
                document,
                CondarcDocument::FellBack(FallbackReason::Rejected)
            ));
        });

        // Then
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].get("reason").map(String::as_str),
            Some("Rejected")
        );
    }

    #[test]
    fn resolve_document_from_valid_file_returns_parsed() {
        // Given
        let contents = "channels: [alpha]\ncreate_default_packages: [numpy]\n";
        let file = write_temp_file(contents.as_bytes());
        let expected = condarc::parse(contents).expect("fixture must parse");

        // When
        let document = resolve_document_from(Some(file.path()));

        // Then
        let config = expect_parsed(document);
        assert_eq!(config, expected);
        assert_eq!(
            config.create_default_packages.as_deref(),
            Some(["numpy".to_string()].as_slice())
        );
    }

    #[test]
    fn consecutive_calls_read_changed_contents_without_caching() {
        // Given
        let file = write_temp_file(b"create_default_packages: [alpha]\n");

        // When
        let first = resolve_document_from(Some(file.path()));
        std::fs::write(file.path(), b"create_default_packages: [beta]\n").unwrap();
        let second = resolve_document_from(Some(file.path()));

        // Then
        assert_eq!(
            expect_parsed(first).create_default_packages.as_deref(),
            Some(["alpha".to_string()].as_slice())
        );
        assert_eq!(
            expect_parsed(second).create_default_packages.as_deref(),
            Some(["beta".to_string()].as_slice())
        );
    }
}
