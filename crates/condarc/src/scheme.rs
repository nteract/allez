//! conda's own `has_scheme()` regex (`^[a-z][a-z0-9]{0,11}://`), factored into its own module so
//! neither `expand_channels` (channel-entry resolution, FR-001(a)) nor `validate` (`channel_alias`
//! validation, FR-025) depends on the other just to reach it.

/// Matches conda's own `has_scheme()` regex, `^[a-z][a-z0-9]{0,11}://` — anchored at the very
/// start of `entry`, never merely somewhere inside it — deriving directly from
/// `docs/condarc_research.md` §8 item 3 (research.md R5), not re-derived. Greedily consuming as
/// many `[a-z0-9]` characters as the `{0,11}` bound allows and then checking for a literal
/// `://` immediately after is equivalent to the regex: `:` is not itself a valid scheme
/// character, so no backtracking is ever needed, and a `://` appearing later in `entry` (e.g.
/// `wrong/scheme://`) can never satisfy this anchored check. Always rejects an empty string —
/// see [`is_valid_channel_alias`] for `channel_alias`'s own, empty-accepting variant of this
/// same rule.
pub(crate) fn has_scheme(entry: &str) -> bool {
    let bytes = entry.as_bytes();
    let Some(first) = bytes.first() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }

    let mut index = 1;
    while index < bytes.len()
        && index < 12
        && (bytes[index].is_ascii_lowercase() || bytes[index].is_ascii_digit())
    {
        index += 1;
    }
    bytes[index..].starts_with(b"://")
}

/// `channel_alias`'s own acceptance rule (FR-025): identical to [`has_scheme`], except an empty
/// string is also accepted, since `channel_alias` may be explicitly set to `""` (FR-018) unlike
/// an ordinary channel-list entry, which `has_scheme` always rejects when empty.
pub(crate) fn is_valid_channel_alias(value: &str) -> bool {
    value.is_empty() || has_scheme(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_scheme_rejects_a_scheme_like_marker_that_starts_later_in_the_entry() {
        // "wrong/scheme://" contains "://" but not immediately after the leading run of
        // `[a-z0-9]` characters, so the anchored check must reject it, not merely check
        // whether "://" appears anywhere in the string.
        assert!(!has_scheme("wrong/scheme://"));
    }

    #[test]
    fn has_scheme_accepts_a_twelve_character_scheme_exactly() {
        // `[a-z][a-z0-9]{0,11}` allows up to 12 total scheme characters.
        assert!(has_scheme("abcdefghijkl://host"));
    }

    #[test]
    fn has_scheme_rejects_a_scheme_longer_than_twelve_characters() {
        // 13-character scheme -- one over the `{0,11}` (max 12 total) bound.
        assert!(!has_scheme("abcdefghijklm://host"));
    }

    #[test]
    fn is_valid_channel_alias_accepts_empty_string_but_has_scheme_does_not() {
        assert!(!has_scheme(""));
        assert!(is_valid_channel_alias(""));
    }
}
