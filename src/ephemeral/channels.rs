use std::collections::VecDeque;

/// Character count at which a redacted value is truncated before appending
/// `...`. A `create_default_packages` entry is never validated before it
/// reaches an error message, so it can be arbitrarily long; truncating keeps
/// one bad entry from flooding a log or terminal.
const MAX_REDACTED_LEN: usize = 512;

/// Query-parameter keys whose values are credentials in practice, including
/// the signed-URL conventions of major cloud storage providers (AWS `X-Amz-*`,
/// Azure SAS `sv`/`se`/`sp`/`sr`/`st`, GCP `X-Goog-*`) and common OAuth token
/// spellings, in addition to generic auth-token/password names.
const CREDENTIAL_QUERY_KEYS: &[&str] = &[
    "token",
    "access_token",
    "refresh_token",
    "id_token",
    "session_token",
    "auth",
    "apikey",
    "api_key",
    "x-api-key",
    "bearer",
    "password",
    "passwd",
    "secret",
    "client_secret",
    "credential",
    "credentials",
    "sig",
    "signature",
    "x-amz-signature",
    "x-amz-security-token",
    "x-amz-credential",
    "x-goog-signature",
    "x-goog-credential",
    "awsaccesskeyid",
    "googleaccessid",
    "sv",
    "se",
    "sp",
    "sr",
    "st",
];

/// Removes credentials from an arbitrary, possibly unvalidated string.
///
/// `CredentialTokenizer` walks the input once and moves between scanning,
/// authority, path, and query states. Percent escapes are collapsed while
/// retaining whether each resulting character was literal or decoded. Literal
/// delimiters are always structural; decoded delimiters and colons can expose
/// hidden query, token, and authority structure, but cannot terminate an
/// active credential span. Userinfo `@` and component `/` terminators remain
/// literal-only. The tokenizer keeps only the two most recent normalized path
/// components uncommitted: that is enough to recognize `://`, remove authority
/// userinfo, and discard a `t` component plus its successor before either can
/// hide or synthesize structure. Every credential decision is therefore made
/// by one traversal rather than by ordering several whole-string rewrites.
///
/// Control characters are escaped after tokenization so crafted diagnostics
/// cannot affect a terminal, and the resulting value is length-bounded.
pub fn redact_channel_url(value: &str) -> String {
    let redacted = CredentialTokenizer::redact(value);
    truncate(&escape_control_characters(&redacted))
}

/// Structural state for the component currently being read.
///
/// `Scanning` covers the leading component, `InPath` covers ordinary
/// slash-delimited components, `InAuthority` records the latest literal `@`
/// while buffering until an authority boundary, and `InQuery` prevents later
/// `@` characters from being mistaken for authority userinfo after `?` or
/// `#`.
#[derive(Clone, Copy)]
enum TraversalState {
    Scanning,
    InAuthority,
    InPath,
    InQuery,
}

#[derive(Clone, Copy)]
struct DecodedCharacter {
    value: char,
    literal: bool,
}

impl DecodedCharacter {
    const fn literal(value: char) -> Self {
        Self {
            value,
            literal: true,
        }
    }

    const fn decoded(value: char) -> Self {
        Self {
            value,
            literal: false,
        }
    }

    const fn is_literal(self, value: char) -> bool {
        self.literal && self.value == value
    }
}

/// Query output is streamed independently of path structure. A key is held
/// until `=` decides whether the following value is copied or discarded.
/// Literal or decoded delimiters can open a key or end a copied value, but only
/// literal delimiters can end a credential value that is being discarded.
enum QueryState {
    InKey,
    InValue,
    RedactingValue,
}

struct QueryWriter {
    output: String,
    key: String,
    state: QueryState,
    written_components: usize,
}

impl QueryWriter {
    fn new(capacity: usize) -> Self {
        Self {
            output: String::with_capacity(capacity),
            key: String::new(),
            state: QueryState::InKey,
            written_components: 0,
        }
    }

    fn write_component(&mut self, component: &[DecodedCharacter]) {
        if self.written_components > 0 {
            self.write_character(DecodedCharacter::literal('/'));
        }
        for &character in component {
            self.write_character(character);
        }
        self.written_components += 1;
    }

    fn write_character(&mut self, character: DecodedCharacter) {
        if matches!(character.value, '?' | '&' | ';' | '#')
            && (character.literal || matches!(self.state, QueryState::InKey | QueryState::InValue))
        {
            if matches!(self.state, QueryState::InKey) {
                self.output.push_str(&self.key);
                self.key.clear();
            }
            self.output.push(character.value);
            self.state = QueryState::InKey;
            return;
        }

        match self.state {
            QueryState::InKey if character.value == '=' => {
                let credential = is_credential_key(&self.key);
                self.output.push_str(&self.key);
                self.output.push('=');
                self.key.clear();
                if credential {
                    self.output.push_str("<redacted>");
                    self.state = QueryState::RedactingValue;
                } else {
                    self.state = QueryState::InValue;
                }
            }
            QueryState::InKey => self.key.push(character.value),
            QueryState::InValue => self.output.push(character.value),
            QueryState::RedactingValue => {}
        }
    }

    fn finish(mut self) -> String {
        match self.state {
            QueryState::InKey => self.output.push_str(&self.key),
            QueryState::InValue | QueryState::RedactingValue => {}
        }
        self.output
    }
}

struct SourceComponent {
    id: usize,
    ends_with_colon: bool,
    is_empty: bool,
    dangling_authority: bool,
}

struct PendingComponent {
    source_id: usize,
    value: Vec<DecodedCharacter>,
    dangling_authority: bool,
}

/// Single-pass tokenizer for decoded structure and credential-bearing spans.
///
/// The percent stack emits only prefixes that can no longer participate in a
/// nested `%XX` escape and tags each emitted character with its provenance.
/// Literal `/` characters finalize path components; decoded `/` characters
/// remain component text while still exposing a hidden `t/` marker. Source
/// history observes every component, while pending output history observes
/// components after redaction and token removal; consulting both lets the
/// same transition catch an authority that removal would either destroy or
/// synthesize. Pending output retains at most two components between
/// component transitions, the exact look-behind needed for a
/// `component:/ /authority` transition.
struct CredentialTokenizer {
    percent_stack: VecDeque<DecodedCharacter>,
    component: Vec<DecodedCharacter>,
    state: TraversalState,
    userinfo_end: Option<usize>,
    source_authority: bool,
    output_authority: bool,
    source_history: VecDeque<SourceComponent>,
    pending_output: VecDeque<PendingComponent>,
    query_writer: QueryWriter,
    skip_next_component: bool,
    current_source_id: usize,
    retained_components: usize,
}

impl CredentialTokenizer {
    fn redact(value: &str) -> String {
        let mut tokenizer = Self::new(value.len());
        for character in value.chars() {
            tokenizer.push_input(character);
        }
        tokenizer.finish()
    }

    fn new(capacity: usize) -> Self {
        Self {
            percent_stack: VecDeque::new(),
            component: Vec::new(),
            state: TraversalState::Scanning,
            userinfo_end: None,
            source_authority: false,
            output_authority: false,
            source_history: VecDeque::with_capacity(2),
            pending_output: VecDeque::with_capacity(3),
            query_writer: QueryWriter::new(capacity),
            skip_next_component: false,
            current_source_id: 0,
            retained_components: 0,
        }
    }

    fn push_input(&mut self, character: char) {
        self.percent_stack
            .push_back(DecodedCharacter::literal(character));
        self.collapse_percent_escapes();
        self.flush_stable_decoded_prefix();
    }

    fn collapse_percent_escapes(&mut self) {
        loop {
            let length = self.percent_stack.len();
            let (Some(marker), Some(high), Some(low)) = (
                length
                    .checked_sub(3)
                    .and_then(|index| self.percent_stack.get(index)),
                length
                    .checked_sub(2)
                    .and_then(|index| self.percent_stack.get(index)),
                length
                    .checked_sub(1)
                    .and_then(|index| self.percent_stack.get(index)),
            ) else {
                break;
            };
            if marker.value != '%' {
                break;
            }
            let Some(byte) = high
                .value
                .to_digit(16)
                .zip(low.value.to_digit(16))
                .map(|(high, low)| high * 16 + low)
                .and_then(|value| u8::try_from(value).ok())
                .filter(u8::is_ascii)
            else {
                break;
            };
            for _ in 0..3 {
                let _ = self.percent_stack.pop_back();
            }
            self.percent_stack
                .push_back(DecodedCharacter::decoded(char::from(byte)));
        }
    }

    fn flush_stable_decoded_prefix(&mut self) {
        let stable_length = if self.percent_stack.front().map(|character| character.value)
            != Some('%')
            || self.percent_stack.back().is_some_and(|character| {
                character.value != '%' && !character.value.is_ascii_hexdigit()
            }) {
            self.percent_stack.len()
        } else {
            0
        };

        for _ in 0..stable_length {
            if let Some(character) = self.percent_stack.pop_front() {
                self.push_decoded(character);
            }
        }
    }

    fn push_decoded(&mut self, character: DecodedCharacter) {
        if character.is_literal('/') {
            self.finish_component(true);
            self.begin_component();
            return;
        }

        self.component.push(character);
        self.state = match self.state {
            TraversalState::InAuthority if character.is_literal('@') => {
                self.userinfo_end = Some(self.component.len());
                TraversalState::InAuthority
            }
            TraversalState::InAuthority
                if character.literal && matches!(character.value, '?' | '#') =>
            {
                TraversalState::InQuery
            }
            TraversalState::Scanning | TraversalState::InPath
                if matches!(character.value, '?' | '&' | ';' | '#') =>
            {
                TraversalState::InQuery
            }
            TraversalState::Scanning => TraversalState::Scanning,
            TraversalState::InAuthority => TraversalState::InAuthority,
            TraversalState::InPath => TraversalState::InPath,
            TraversalState::InQuery => TraversalState::InQuery,
        };
    }

    fn begin_component(&mut self) {
        self.source_authority = self.source_history.len() == 2
            && self.source_history[0].ends_with_colon
            && self.source_history[1].is_empty;
        if self.source_authority && self.source_history[0].dangling_authority {
            let source_id = self.source_history[0].id;
            if let Some(component) = self
                .pending_output
                .iter_mut()
                .find(|component| component.source_id == source_id)
            {
                component.value.clear();
                component.value.push(DecodedCharacter::literal(':'));
                component.dangling_authority = false;
            }
        }

        self.output_authority = self.pending_output.len() == 2
            && ends_with_value(&self.pending_output[0].value, ':')
            && self.pending_output[1].value.is_empty();
        if self.output_authority && self.pending_output[0].dangling_authority {
            self.pending_output[0].value.clear();
            self.pending_output[0]
                .value
                .push(DecodedCharacter::literal(':'));
            self.pending_output[0].dangling_authority = false;
        }

        self.state = if self.source_authority || self.output_authority {
            TraversalState::InAuthority
        } else if self.current_source_id == 0 && self.retained_components == 0 {
            TraversalState::Scanning
        } else {
            TraversalState::InPath
        };
        self.userinfo_end = None;
    }

    fn finish_component(&mut self, has_following_component: bool) {
        let original = std::mem::take(&mut self.component);
        let source_ends_with_colon = ends_with_value(&original, ':');
        let source_is_empty = original.is_empty();
        let mut userinfo_end = self.userinfo_end;

        if (self.current_source_id == 0 || self.retained_components == 0)
            && let Some(at) = original
                .iter()
                .rposition(|character| character.is_literal('@'))
            && original[..at]
                .iter()
                .any(|character| character.value == ':')
        {
            let leading_end = at + 1;
            userinfo_end = Some(userinfo_end.map_or(leading_end, |end| end.max(leading_end)));
        }

        let mut value = original;
        if let Some(index) = userinfo_end {
            value.drain(..index);
        }
        let source_dangling = dangling_authority(&value, self.source_authority);
        let token_marker = value.iter().enumerate().find_map(|(index, character)| {
            let starts_marker = index == 0 || value[index - 1].value == '/';
            let ends_marker = index + 1 == value.len() || value[index + 1].value == '/';
            (character.value == 't' && starts_marker && ends_marker).then_some(index)
        });

        let retain_component = if self.skip_next_component {
            self.skip_next_component = false;
            false
        } else if let Some(index) = token_marker {
            if index > 0 {
                let marker_ends_component = index + 1 == value.len();
                if !marker_ends_component || has_following_component {
                    value.truncate(index - 1);
                }
                if marker_ends_component && has_following_component {
                    self.skip_next_component = true;
                }
                true
            } else if value.len() > 1 {
                false
            } else if has_following_component {
                self.skip_next_component = true;
                false
            } else {
                true
            }
        } else {
            true
        };
        if retain_component {
            let output_dangling = dangling_authority(&value, self.output_authority);
            self.pending_output.push_back(PendingComponent {
                source_id: self.current_source_id,
                value,
                dangling_authority: output_dangling,
            });
            self.retained_components += 1;
            if self.pending_output.len() > 2 {
                self.flush_oldest_component();
            }
        }

        self.source_history.push_back(SourceComponent {
            id: self.current_source_id,
            ends_with_colon: source_ends_with_colon,
            is_empty: source_is_empty,
            dangling_authority: source_dangling,
        });
        if self.source_history.len() > 2 {
            let _ = self.source_history.pop_front();
        }
        self.current_source_id += 1;
    }

    fn flush_oldest_component(&mut self) {
        if let Some(component) = self.pending_output.pop_front() {
            self.query_writer.write_component(&component.value);
        }
    }

    fn finish(mut self) -> String {
        while let Some(character) = self.percent_stack.pop_front() {
            self.push_decoded(character);
        }
        self.finish_component(false);
        while !self.pending_output.is_empty() {
            self.flush_oldest_component();
        }
        self.query_writer.finish()
    }
}

fn ends_with_value(component: &[DecodedCharacter], value: char) -> bool {
    component
        .last()
        .is_some_and(|character| character.value == value)
}

fn dangling_authority(component: &[DecodedCharacter], starts_in_authority: bool) -> bool {
    if !starts_in_authority || !ends_with_value(component, ':') {
        return false;
    }
    let authority = &component[..component.len() - 1];
    !authority
        .iter()
        .any(|character| character.literal && matches!(character.value, '?' | '#' | '@'))
        && authority.iter().any(|character| character.value == ':')
}

fn is_credential_key(key: &str) -> bool {
    let key = key.trim().to_ascii_lowercase();
    CREDENTIAL_QUERY_KEYS
        .iter()
        .any(|candidate| key == *candidate)
}

/// True for a character that can visually disguise text in a terminal or
/// log even though `char::is_control()` does not itself flag it: Unicode
/// bidirectional-override/embedding controls (e.g. U+202E RIGHT-TO-LEFT
/// OVERRIDE), zero-width joiners/marks, and the BOM. `char::is_control()`
/// only covers the Unicode `Cc` category; these are `Cf`-category format
/// characters outside it.
fn is_disguising_format_character(character: char) -> bool {
    matches!(
        character,
        '\u{061C}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'
            | '\u{2066}'..='\u{2069}'
            | '\u{FEFF}'
    )
}

/// Escapes control characters, including ESC, and Unicode
/// bidirectional-override/zero-width characters, so a crafted value cannot
/// inject terminal escape sequences or visually disguise itself (e.g. via a
/// right-to-left override) in human-readable output.
fn escape_control_characters(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_debug().collect::<Vec<_>>()
            } else if is_disguising_format_character(character) {
                format!("\\u{{{:04x}}}", character as u32)
                    .chars()
                    .collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
}

fn truncate(value: &str) -> String {
    if value.chars().count() <= MAX_REDACTED_LEN {
        return value.to_string();
    }
    let kept: String = value.chars().take(MAX_REDACTED_LEN).collect();
    format!("{kept}...")
}

#[cfg(test)]
mod tests {
    use super::{MAX_REDACTED_LEN, redact_channel_url};

    #[test]
    fn redact_channel_url_removes_userinfo_and_conda_tokens_and_preserves_clean_values() {
        assert_eq!(
            redact_channel_url("https://user:password@repo.example/t/token-123/conda-forge"),
            "https://repo.example/conda-forge"
        );
        assert_eq!(
            redact_channel_url("user:password@repo.example/t/token-456/pkg"),
            "repo.example/pkg"
        );
        assert_eq!(
            redact_channel_url("https://repo.example/conda-forge"),
            "https://repo.example/conda-forge"
        );
    }

    #[test]
    fn redact_channel_url_preserves_a_plain_channel_qualified_spec() {
        assert_eq!(
            redact_channel_url("conda-forge::numpy=1.2"),
            "conda-forge::numpy=1.2"
        );
    }

    #[test]
    fn redact_channel_url_removes_userinfo_from_every_embedded_url() {
        // Given: a malformed entry whose first authority is credential-free,
        // so redacting only the leading authority would leak the second.
        let value = "[[[dummy://safe/path/https://user:tokenSECRET@repo.example/chan::pkg";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("tokenSECRET"), "{redacted}");
        assert!(!redacted.contains("user:"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_credential_query_parameters() {
        // Given
        let value = "https://repo.example/chan?token=SECRET123&Password=hunter2&subdir=noarch";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("SECRET123"), "{redacted}");
        assert!(!redacted.contains("hunter2"), "{redacted}");
        assert!(redacted.contains("subdir=noarch"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_escapes_control_characters() {
        // Given: an entry carrying a terminal escape sequence.
        let value = "numpy\u{1b}[2J\u{7}";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains('\u{1b}'), "{redacted}");
        assert!(!redacted.contains('\u{7}'), "{redacted}");
    }

    #[test]
    fn redact_channel_url_bounds_an_oversized_value() {
        // Given
        let value = "n".repeat(MAX_REDACTED_LEN * 2);

        // When
        let redacted = redact_channel_url(&value);

        // Then
        assert_eq!(redacted.chars().count(), MAX_REDACTED_LEN + 3);
        assert!(redacted.ends_with("..."));
    }

    #[test]
    fn redact_channel_url_removes_credentials_from_a_later_embedded_query() {
        // Given: an earlier credential-free query would end the scan if only
        // the first `?` were parsed.
        let value = "https://a.example/?x=1 https://b.example/?token=SECRET123";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("SECRET123"), "{redacted}");
        assert!(redacted.contains("x=1"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_semicolon_separated_credentials() {
        // Given
        let value = "https://host/p?x=1;token=SECRET123";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("SECRET123"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_repeated_question_mark_credentials() {
        // Given
        let value = "https://host/p?x=1?token=SECRET123";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("SECRET123"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_signed_url_credential_keys() {
        // Given
        let keys = [
            "access_token",
            "auth",
            "apikey",
            "api_key",
            "bearer",
            "sig",
            "signature",
        ];

        for key in keys {
            // When
            let redacted = redact_channel_url(&format!("https://host/p?{key}=SECRET123"));

            // Then
            assert!(!redacted.contains("SECRET123"), "{key}: {redacted}");
        }
    }

    #[test]
    fn redact_channel_url_preserves_a_benign_key_query_parameter() {
        // Given: `key` alone is too generic to treat as a credential.
        let value = "https://repo.example/chan?key=noarch";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert_eq!(redacted, value);
    }

    #[test]
    fn redact_channel_url_removes_a_credential_written_after_a_fragment_marker() {
        // Given: a query-looking credential placed after `#`, which an
        // earlier version of this function copied through unredacted.
        let value = "bundle?x=1#access_token=TOPSECRET";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
        assert!(redacted.contains("x=1"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_credential_after_a_fragment_marker_with_no_query_at_all() {
        // Given: no `?` anywhere in the value, only a `#`.
        let value = "bundle#token=TOPSECRET";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_credential_after_a_leading_ampersand_with_no_query_marker() {
        // Given: no `?`/`#` anywhere, only a leading `&` — an earlier
        // version of this function's early-return guard checked `?`/`#`
        // only and skipped the scan entirely for a value like this.
        let value = "bundle&access_token=TOPSECRET";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_credential_after_a_leading_semicolon_with_no_query_marker() {
        // Given
        let value = "bundle;token=TOPSECRET";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_oauth_token_credential_keys() {
        // Given
        let keys = ["refresh_token", "id_token", "session_token"];

        for key in keys {
            // When
            let redacted = redact_channel_url(&format!("https://host/p?{key}=SECRET123"));

            // Then
            assert!(!redacted.contains("SECRET123"), "{key}: {redacted}");
        }
    }

    #[test]
    fn redact_channel_url_removes_cloud_credential_identifier_keys() {
        // Given
        let keys = ["x-goog-credential", "awsaccesskeyid", "googleaccessid"];

        for key in keys {
            // When
            let redacted = redact_channel_url(&format!("https://host/p?{key}=SECRET123"));

            // Then
            assert!(!redacted.contains("SECRET123"), "{key}: {redacted}");
        }
    }

    #[test]
    fn redact_channel_url_removes_userinfo_from_a_second_url_concatenated_without_a_separator() {
        // Given: the first authority's own text ("safehttps") is itself the
        // start of a second `scheme://`, with no `/`/`?`/`#` between them —
        // an earlier version folded that second scheme's marker into the
        // first authority as inert text, consuming the boundary the second
        // `while`-loop iteration needed to find and strip its own userinfo.
        let value = "http://safehttps://user:SECRET@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("SECRET"), "{redacted}");
        assert!(!redacted.contains("user:"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_userinfo_that_precedes_a_concatenated_scheme() {
        // Given: the credential-bearing text sits BEFORE the concatenated
        // scheme this time, so its own `@` terminator belongs to the
        // second, nested authority rather than to this text -- an earlier
        // fix handled only the reverse ordering and let this survive.
        let value = "http://user:TOPSECREThttps://safe@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_credential_that_opens_the_value_with_no_preceding_text() {
        // Given: nothing precedes the first delimiter for this value to
        // "be" -- an earlier version treated the segment before the first
        // delimiter as always-safe base-URL text and never checked it.
        let value = "token=TOPSECRET&x=1";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
        assert!(redacted.contains("x=1"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_bare_credential_pair_with_no_delimiter_at_all() {
        // Given: no `?`/`&`/`;`/`#` anywhere -- an earlier version's
        // early-return guard skipped the scan entirely whenever no
        // delimiter was present, even though a value can be nothing but a
        // single, delimiter-free `key=value` credential pair.
        let value = "token=TOPSECRET";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_userinfo_synthesized_by_removing_a_token_path_segment() {
        // Given: removing the `/t/secret/` segment rejoins the surrounding
        // text into a brand-new `http://user:PASS@host` authority that did
        // not exist before that removal -- an earlier version ran userinfo
        // redaction *before* token-path redaction, so this newly-created
        // authority was never seen by the userinfo pass at all.
        let value = "http:/t/secret//user:PASS@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("PASS"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_userinfo_that_token_path_removal_would_otherwise_destroy() {
        // Given: `t` and its own next component, `http:`, are exactly what
        // token-path removal drops here, breaking a REAL, already
        // credential-bearing `http://user:PASS@host` authority into
        // `prefix//user:PASS@host` (no `://` left at all) -- an earlier
        // version ran userinfo redaction only *after* token-path removal,
        // so the authority was already destroyed by the time it ran.
        let value = "prefix/t/http://user:PASS@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("PASS"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_credential_only_a_second_round_of_removal_exposes() {
        // Given: the first userinfo/token-path round leaves a fresh
        // `/t/TOPSECRET/` shape that the fixed-point loop's SECOND round
        // must remove -- a version that only ran the userinfo/token-path
        // sandwich once (not to a fixed point) stopped after the first
        // round and left this behind.
        let value = "http:/t/drop//user:pass@t/TOPSECRET/pkg";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_credential_hidden_behind_a_percent_encoded_path_delimiter() {
        // Given: every `/` inside the token-path segment is percent-encoded
        // (`%2F`/doubly-encoded `%252F`), so splitting on a literal `/`
        // before decoding never separates it into a component that equals
        // `t` at all -- an earlier version decoded each already-split
        // component instead of decoding the whole value first, so the
        // encoded delimiters hid the entire span as one opaque component.
        let value = "prefixhttp:/%2574%252FTOPSECRET%252F%252Fuser:PASShttps://safe@host?%2561ccess_token=QUERY";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
        assert!(!redacted.contains("PASS"), "{redacted}");
        assert!(!redacted.contains("QUERY"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_double_percent_encoded_credential_query_key() {
        // Given: `%2561ccess_token` decodes once to the still-encoded
        // `%61ccess_token`, not all the way to `access_token` -- an earlier
        // version's single-pass decode stopped there.
        let value = "https://host/p?%2561ccess_token=SECRET123";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("SECRET123"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_double_percent_encoded_token_path_segment() {
        // Given: `%2574` decodes once to the still-encoded `%74`, not all
        // the way to `t`.
        let value = "https://repo.example/%2574/token-123/pkg";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("token-123"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_nine_layer_percent_encoded_credential_query_key() {
        // Given: 8 layers of `%25` (each decoding to one more literal `%`)
        // in front of a final `61`, needing 9 decode passes total to fully
        // resolve to `access_token` -- an earlier version's fixed 8-pass
        // cap left this exact depth still encoded and unmatched.
        let key = format!("%{}61ccess_token", "25".repeat(8));
        let value = format!("https://host/p?{key}=SECRET123");

        // When
        let redacted = redact_channel_url(&value);

        // Then
        assert!(!redacted.contains("SECRET123"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_nine_layer_percent_encoded_token_path_segment() {
        // Given: the same 9-layer nesting depth, this time encoding a
        // conda token path's `t` component (`74`) instead of a query key.
        let component = format!("%{}74", "25".repeat(8));
        let value = format!("https://repo.example/{component}/token-nine/pkg");

        // When
        let redacted = redact_channel_url(&value);

        // Then
        assert!(!redacted.contains("token-nine"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_percent_encoded_credential_query_key() {
        // Given: `%61ccess_token` decodes to `access_token`, dodging an
        // earlier version's exact-string key match.
        let value = "https://host/p?%61ccess_token=SECRET123";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("SECRET123"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_credential_key_with_a_percent_encoded_underscore() {
        // Given: `%5f` decodes to `_`.
        let value = "https://host/p?access%5ftoken=SECRET123";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("SECRET123"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_a_percent_encoded_token_path_segment() {
        // Given: `%74` decodes to `t`, dodging an earlier version's exact
        // `component == "t"` path-segment match.
        let value = "https://repo.example/%74/token-123/pkg";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("token-123"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_cloud_signed_url_credential_keys() {
        // Given
        let keys = [
            "client_secret",
            "credential",
            "credentials",
            "x-amz-signature",
            "x-amz-security-token",
            "x-amz-credential",
            "x-goog-signature",
            "x-api-key",
            "sv",
            "se",
            "sp",
            "sr",
            "st",
        ];

        for key in keys {
            // When
            let redacted = redact_channel_url(&format!("https://host/p?{key}=SECRET123"));

            // Then
            assert!(!redacted.contains("SECRET123"), "{key}: {redacted}");
        }
    }

    #[test]
    fn redact_channel_url_ignores_a_percent_encoded_authority_delimiter_inside_userinfo() {
        // Given: `%2F` is credential text, not a literal authority boundary;
        // losing that provenance previously exposed the complete userinfo.
        let value = "https://user:TOPSECRET%2Fjunk@repo.example/path";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
        assert!(!redacted.contains("user:"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_keeps_dropping_a_token_across_a_percent_encoded_path_delimiter() {
        // Given: `%2F` is part of the token component, so it must not split
        // the token and expose its suffix as a new path component.
        let value = "https://repo.example/t/junk%2FTOPSECRET/pkg";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_keeps_redacting_a_query_value_across_an_encoded_delimiter() {
        // Given: `%26` belongs to the credential value; treating its decoded
        // `&` as a parameter boundary previously exposed the value suffix.
        let value = "https://host/p?token=SECRET%26TAIL&x=1";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TAIL"), "{redacted}");
        assert!(redacted.contains("x=1"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_credentials_from_a_composite_encoded_decoy_input() {
        // Given: nested encoding combines a hidden token path, synthesized
        // authority, encoded userinfo delimiter, and encoded query key.
        let value = "prefixhttp:/%2574/drop/%252Fhttps://user:TOPSECRET%252Fjunk@host?%2561ccess_token=QUERY";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
        assert!(!redacted.contains("QUERY"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_redacts_a_fully_percent_encoded_query_credential() {
        // Given: decoded `?` and `=` must open query structure even though
        // an earlier literal-only check treated the credential as inert text.
        let value = "https://host/p%3Faccess_token%3DTOPSECRET";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert_eq!(redacted, "https://host/p?access_token=<redacted>");
    }

    #[test]
    fn redact_channel_url_removes_scheme_less_userinfo_with_an_encoded_colon() {
        // Given: the decoded colon is credential evidence, but an earlier
        // literal-only heuristic failed to recognize the leading userinfo.
        let value = "user%3ATOPSECRET@repo.example/path";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert_eq!(redacted, "repo.example/path");
    }

    #[test]
    fn redact_channel_url_removes_nested_userinfo_with_an_encoded_colon() {
        // Given: the decoded colon proves the first authority is dangling;
        // requiring literal evidence previously exposed its credential.
        let value = "http://user%3ATOPSECREThttps://safe@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_encoded_colon_userinfo_from_a_composite_decoy() {
        // Given: token removal and encoded decoys expose a dangling authority
        // whose credential-indicating colon was previously ignored.
        let value = "prefix/t/drop/http://user%253ATOPSECREThttps://safe%2540decoy@host?token=QUERY%2526TAIL&x=1";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
        assert!(!redacted.contains("QUERY"), "{redacted}");
        assert!(!redacted.contains("TAIL"), "{redacted}");
        assert!(redacted.contains("x=1"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_an_embedded_token_marker_after_decoded_text() {
        // Given: a decoded slash places `t/` after an unrelated prefix;
        // matching only at offset zero previously copied the token verbatim.
        let value = "https://repo.example/junk%2Ft%2FOWN_EMBEDDED_TOKEN/pkg";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert_eq!(redacted, "https://repo.example/junk/pkg");
    }

    #[test]
    fn redact_channel_url_removes_userinfo_before_a_double_encoded_scheme_colon() {
        // Given: encoded userinfo and scheme colons hid the nested authority,
        // so the first authority's credential previously remained visible.
        let value = "http://user%3ATOPSECREThttps%3A//safe@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_userinfo_after_an_encoded_scheme_colon() {
        // Given: the encoded scheme colon prevented authority state from
        // opening before the credential that followed it.
        let value = "http://safehttp%3A//user:CRED@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("CRED"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_userinfo_before_an_encoded_scheme_colon() {
        // Given: a credential before an encoded nested-scheme colon was
        // retained because the new authority boundary was never recognized.
        let value = "http://user:CREDhttp%3A//safe@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("CRED"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_removes_userinfo_from_a_fully_encoded_three_scheme_chain() {
        // Given: encoded scheme and credential colons across three chained
        // authorities must not hide either credential span.
        let value = "a%3A//u1%3AP1b%3A//u2%3AP2c%3A//safe@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert_eq!(redacted, "a://://://host");
    }

    #[test]
    fn redact_channel_url_removes_userinfo_from_a_mixed_colon_three_scheme_chain() {
        // Given: alternating literal and encoded scheme and credential colons
        // across three chained authorities must not expose either credential.
        let value = "a%3A//u1:P1b://u2%3AP2c%3A//safe@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("P1"), "{redacted}");
        assert!(!redacted.contains("P2"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_drops_a_successor_after_an_embedded_terminal_token_marker() {
        // Given: a decoded `/t` marker ended its component, so the literal
        // successor token previously remained visible.
        let value = "https://repo.example/junk%2Ft/OWN_EMBEDDED_TOKEN/pkg";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert_eq!(redacted, "https://repo.example/junk/pkg");
    }

    #[test]
    fn redact_channel_url_opens_a_credential_after_an_encoded_safe_value_delimiter() {
        // Given: a decoded `&` follows a safe value and introduces a real
        // credential key that was previously copied as inert value text.
        let value = "https://host/p%3Fx%3D1%26access_token%3DTOPSECRET";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert_eq!(redacted, "https://host/p?x=1&access_token=<redacted>");
    }

    #[test]
    fn redact_channel_url_removes_userinfo_from_an_authority_synthesized_after_an_encoded_colon() {
        // Given: decoding the scheme colon and removing the token path creates
        // an authority whose userinfo did not exist in the source structure.
        let value = "http%3A/t/secret//user:PASS@host";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert_eq!(redacted, "http://host");
    }

    #[test]
    fn redact_channel_url_removes_composite_synthesized_authority_and_query_credentials() {
        // Given: token removal synthesizes an authority after an encoded colon
        // while a credential-bearing query follows it.
        let value = "http%3A/t/secret//user:CRED@host?token=TOPSECRET";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains("CRED"), "{redacted}");
        assert!(!redacted.contains("TOPSECRET"), "{redacted}");
    }

    #[test]
    fn redact_channel_url_escapes_a_right_to_left_override_character() {
        // Given: U+202E can visually reverse/disguise the text that follows
        // it in a terminal or log, and is not `char::is_control()`.
        let value = "numpy\u{202e}pypmun";

        // When
        let redacted = redact_channel_url(value);

        // Then
        assert!(!redacted.contains('\u{202e}'), "{redacted}");
        assert!(redacted.contains("\\u{202e}"), "{redacted}");
    }

    #[test]
    fn resolved_channels_from_channels_defaults_policy() {
        let config = condarc::ResolvedChannels::from_channels(vec!["conda-forge".to_string()]);

        assert_eq!(config.channel_priority, condarc::ChannelPriority::Strict);
        assert_eq!(config.channels, vec!["conda-forge"]);
    }
}
