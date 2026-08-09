//! Asserts `skills/allez-oneshot.md` documents GEN-30's default-package
//! source, the empty-by-default outcome, and the precedence rule (FR-005).
//! A plain file read: no Cargo feature, no compiled `allez` binary, no
//! `[[test]]` entry needed.

/// The `### Default packages` subsection's own body, from its heading up
/// to its next sibling heading, with Markdown emphasis and code markers
/// stripped so an assertion tests the documented fact rather than the
/// formatting that happens to surround it.
fn default_packages_section(document: &str) -> String {
    let start = document
        .find("### Default packages")
        .expect("skills/allez-oneshot.md must have a `### Default packages` subsection");
    let body = &document[start..];
    let section = match body[1..].find("\n### ") {
        Some(offset) => &body[..=offset],
        None => body,
    };
    section.replace(['*', '`'], "")
}

fn section() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("skills/allez-oneshot.md");
    let document = std::fs::read_to_string(&path).unwrap();
    default_packages_section(&document)
}

#[test]
fn skills_doc_names_condarc_create_default_packages_as_the_default_source() {
    // Given/When
    let section = section();

    // Then
    assert!(
        section.contains("create_default_packages"),
        "the `### Default packages` subsection must name the `create_default_packages` \
         setting: {section}"
    );
    assert!(
        section.contains("~/.condarc"),
        "the `### Default packages` subsection must name `~/.condarc` as the source: {section}"
    );
}

#[test]
fn skills_doc_states_an_unconfigured_condarc_installs_nothing_including_python() {
    // Given/When
    let section = section();

    // Then
    assert!(
        section.contains("no packages at all"),
        "the `### Default packages` subsection must state that an unconfigured \
         `create_default_packages` yields no packages at all: {section}"
    );
    assert!(
        section.contains("not even python"),
        "the `### Default packages` subsection must state that not even `python` is included \
         by default: {section}"
    );
}

#[test]
fn skills_doc_states_the_add_and_supersede_precedence() {
    // Given/When
    let section = section();

    // Then
    assert!(
        section.contains("are added to that resolved default set"),
        "the `### Default packages` subsection must state that named packages are added to the \
         resolved default set: {section}"
    );
    assert!(
        section.contains("matches a default-set entry replaces that one entry"),
        "the `### Default packages` subsection must state that a matching bare name replaces \
         that default-set entry: {section}"
    );
}
