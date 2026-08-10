//! `create_default_packages` extraction (GEN-30 FR-001/FR-002): the
//! ephemeral-environment default package set is exactly whatever the
//! caller's own `.condarc` resolved it to, with no `allez`-authored
//! fallback list, defaulting, or validation of any kind.

use crate::channel_config::CondarcDocument;
use crate::ephemeral::PackageSpec;

/// Resolves the default package set from an already-resolved `.condarc`
/// document: a parsed document's own `create_default_packages`, or an
/// empty `Vec` for an absent or fallen-back one. Cannot fail — every
/// resolved entry, however malformed, is passed through unchanged for
/// the merge and the solver to judge later.
pub(crate) fn create_default_packages_from_document(
    document: &CondarcDocument,
) -> Vec<PackageSpec> {
    match document {
        CondarcDocument::Parsed(config) => config
            .create_default_packages
            .clone()
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, entry)| PackageSpec::from_resolved_default(entry, index))
            .collect(),
        CondarcDocument::Absent | CondarcDocument::FellBack(_) => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::create_default_packages_from_document;
    use crate::channel_config::{CondarcDocument, FallbackReason};

    fn parsed_with(create_default_packages: Option<Vec<String>>) -> CondarcDocument {
        let mut config = condarc::Config::default();
        config.create_default_packages = create_default_packages;
        CondarcDocument::Parsed(Box::new(config))
    }

    fn resolved_strings(document: &CondarcDocument) -> Vec<String> {
        create_default_packages_from_document(document)
            .iter()
            .map(|package| package.as_str().to_string())
            .collect()
    }

    #[test]
    fn create_default_packages_from_document_absent_falls_back_to_empty() {
        assert!(create_default_packages_from_document(&CondarcDocument::Absent).is_empty());
    }

    #[test]
    fn create_default_packages_from_document_unreadable_falls_back_to_empty() {
        // Given
        let document = CondarcDocument::FellBack(FallbackReason::Unreadable);

        // When/Then
        assert!(create_default_packages_from_document(&document).is_empty());
    }

    #[test]
    fn create_default_packages_from_document_rejected_falls_back_to_empty() {
        // Given
        let document = CondarcDocument::FellBack(FallbackReason::Rejected);

        // When/Then
        assert!(create_default_packages_from_document(&document).is_empty());
    }

    #[test]
    fn create_default_packages_from_document_parsed_none_resolves_to_empty() {
        assert!(create_default_packages_from_document(&parsed_with(None)).is_empty());
    }

    #[test]
    fn create_default_packages_from_document_parsed_empty_list_resolves_to_empty() {
        assert!(create_default_packages_from_document(&parsed_with(Some(Vec::new()))).is_empty());
    }

    #[test]
    fn create_default_packages_from_document_empty_string_entry_resolves_successfully() {
        // Given
        let document = parsed_with(Some(vec![String::new()]));

        // When/Then
        assert_eq!(resolved_strings(&document), vec![String::new()]);
    }

    #[test]
    fn create_default_packages_from_document_whitespace_only_entry_resolves_successfully() {
        // Given
        let document = parsed_with(Some(vec!["   ".to_string()]));

        // When/Then
        assert_eq!(resolved_strings(&document), vec!["   ".to_string()]);
    }

    #[test]
    fn create_default_packages_from_document_arbitrary_string_entry_resolves_successfully() {
        // Given
        let document = parsed_with(Some(vec!["[[[not a spec".to_string()]));

        // When/Then
        assert_eq!(
            resolved_strings(&document),
            vec!["[[[not a spec".to_string()]
        );
    }
}
