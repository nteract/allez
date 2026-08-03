//! Resolves parsed `.condarc` channel preferences into concrete channel identifiers.

use std::collections::HashMap;

use crate::model::{ChannelPriority, Config};
use crate::scheme::has_scheme;

const DEFAULT_CHANNEL_ALIAS: &str = "https://conda.anaconda.org";
#[cfg(not(windows))]
const DEFAULT_CHANNELS: &[&str] = &[
    "https://repo.anaconda.com/pkgs/main",
    "https://repo.anaconda.com/pkgs/r",
];
#[cfg(windows)]
const DEFAULT_CHANNELS: &[&str] = &[
    "https://repo.anaconda.com/pkgs/main",
    "https://repo.anaconda.com/pkgs/r",
    "https://repo.anaconda.com/pkgs/msys2",
];
const DEFAULT_CUSTOM_CHANNELS: &[(&str, &str)] = &[("pkgs/pro", "https://repo.anaconda.com")];

/// The complete result of successfully resolving one parsed [`Config`]'s
/// channel settings (FR-001–FR-004, FR-005–FR-008, FR-019): an ordered,
/// fully-expanded, already deny-then-allow filtered channel list, and
/// the effective channel-priority mode. The separate allow-list/deny-list
/// entries used to compute `channels` are not themselves part of this
/// type (research.md R12) — they exist only inside `expand_channels()`'s
/// own implementation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedChannels {
    /// Ordered, concrete channel identifiers (see Concrete Channel
    /// Identifier in spec.md), preserving `channels`'/`default_channels`'
    /// own original relative ordering throughout every expansion, with
    /// FR-019's deny-then-allow filtering already applied. Can
    /// legitimately be empty — see spec.md Design Decisions, "Empty
    /// resolved list, two legitimate causes."
    pub channels: Vec<String>,
    /// Mirrors `Config::channel_priority`, defaulted to `Flexible` when
    /// absent (FR-002/FR-003). Never itself re-derives the legacy
    /// boolean-spelling mapping — that already happened at `parse()` time
    /// (research.md R2).
    pub channel_priority: ChannelPriority,
}

impl ResolvedChannels {
    /// Builds an unrestricted, strict-priority `ResolvedChannels` from
    /// already-resolved channel identifiers, bypassing
    /// `expand_channels()` entirely — mirrors the retired
    /// `allez::ephemeral::ChannelConfig::from_urls`'s own behavior
    /// exactly (research.md R15). Needed because `ResolvedChannels` is
    /// `#[non_exhaustive]`, which blocks an external crate (`allez`'s
    /// own tests, `examples/ephemeral_smoke.rs`) from constructing one
    /// via struct literal.
    pub fn from_channels(channels: Vec<String>) -> Self {
        Self {
            channels,
            channel_priority: ChannelPriority::Strict,
        }
    }
}

/// Why `expand_channels()` could not produce a `ResolvedChannels` at all
/// (research.md R11). `#[non_exhaustive]` for the same future-proofing
/// reason every other public enum in this ticket's scope already uses —
/// today this has exactly one inhabited variant.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandChannelsError {
    /// Resolving `entry` required joining it to `channel_alias`
    /// (FR-001(d), or the restricted `resolve_member` precedence), but
    /// the effective `channel_alias` is an explicit empty string
    /// (FR-018). `entry` is the literal channel-list entry (or
    /// multichannel member) that triggered this, for the caller's own
    /// error message/observability detail.
    EmptyChannelAlias {
        /// The literal channel-list entry or multichannel member that
        /// required the empty alias.
        entry: String,
    },
}

impl std::fmt::Display for ExpandChannelsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyChannelAlias { entry } => write!(
                f,
                "cannot resolve {entry:?}: channel_alias is an explicit empty string"
            ),
        }
    }
}

impl std::error::Error for ExpandChannelsError {}

struct ResolveContext<'a> {
    channel_alias: &'a str,
    custom_channels: HashMap<&'a str, &'a str>,
    custom_multichannels: HashMap<&'a str, &'a [String]>,
    default_channels: Vec<&'a str>,
}

fn match_custom_channel(entry: &str, custom_channels: &HashMap<&str, &str>) -> Option<String> {
    let mut prefix = entry;
    loop {
        if let Some(base_url) = custom_channels.get(prefix) {
            return Some(format!("{}/{entry}", base_url.trim_end_matches('/')));
        }
        let (parent, _) = prefix.rsplit_once('/')?;
        prefix = parent;
    }
}

fn resolve_member(entry: &str, channel_alias: &str) -> Result<String, ExpandChannelsError> {
    if has_scheme(entry) {
        return Ok(entry.to_string());
    }
    if channel_alias.is_empty() {
        return Err(ExpandChannelsError::EmptyChannelAlias {
            entry: entry.to_string(),
        });
    }

    Ok(format!("{}/{entry}", channel_alias.trim_end_matches('/')))
}

fn resolve_entry(
    entry: &str,
    ctx: &ResolveContext<'_>,
) -> Result<Vec<String>, ExpandChannelsError> {
    if has_scheme(entry) {
        return Ok(vec![entry.to_string()]);
    }
    if let Some(members) = ctx.custom_multichannels.get(entry) {
        return members
            .iter()
            .map(|member| resolve_member(member, ctx.channel_alias))
            .collect();
    }
    if entry == "defaults" {
        return ctx
            .default_channels
            .iter()
            .map(|member| resolve_member(member, ctx.channel_alias))
            .collect();
    }
    if let Some(channel) = match_custom_channel(entry, &ctx.custom_channels) {
        return Ok(vec![channel]);
    }

    Ok(vec![resolve_member(entry, ctx.channel_alias)?])
}

fn resolve_role(
    role_entries: &[&str],
    ctx: &ResolveContext<'_>,
) -> Result<Vec<String>, ExpandChannelsError> {
    let mut channels = Vec::new();
    for entry in role_entries {
        channels.extend(resolve_entry(entry, ctx)?);
    }
    Ok(channels)
}

fn apply_allow_deny(
    channels: Vec<String>,
    allowlist: &[String],
    denylist: &[String],
) -> Vec<String> {
    let mut filtered = channels;
    filtered.retain(|channel| !denylist.contains(channel));
    if !allowlist.is_empty() {
        filtered.retain(|channel| allowlist.contains(channel));
    }
    filtered
}

/// Resolves one parsed `.condarc` [`Config`]'s channel settings
/// (`channels`/`channel`, `channel_alias`, `custom_channels`,
/// `custom_multichannels`, `default_channels`, `channel_priority`,
/// `allowlist_channels`/`whitelist_channels`, `denylist_channels`) into a
/// single, ordered, fully-expanded, already-filtered [`ResolvedChannels`]
/// — see FR-001 through FR-004 and FR-005 through FR-008, FR-018, FR-019.
/// Layered on top of, and never mutating, [`crate::parse`]'s output.
///
/// Performs no I/O of any kind: a pure function of `config`, applying
/// conda's own documented defaults (research.md R4) wherever a relevant
/// setting is absent, exactly as FR-002's table specifies. Returns
/// `Err(ExpandChannelsError::EmptyChannelAlias)` only when resolving some
/// entry actually requires an empty-`channel_alias` join (FR-018,
/// research.md R11); every other input, including one whose allow/deny
/// filtering (FR-019) legitimately empties `channels`, resolves
/// successfully.
pub fn expand_channels(config: &Config) -> Result<ResolvedChannels, ExpandChannelsError> {
    let channel_alias = config
        .channel_alias
        .as_deref()
        .unwrap_or(DEFAULT_CHANNEL_ALIAS);
    let custom_channels = match &config.custom_channels {
        Some(channels) => channels
            .iter()
            .map(|(name, base_url)| (name.as_str(), base_url.as_str()))
            .collect(),
        None => DEFAULT_CUSTOM_CHANNELS.iter().copied().collect(),
    };
    let custom_multichannels = match &config.custom_multichannels {
        Some(multichannels) => multichannels
            .iter()
            .map(|(name, members)| (name.as_str(), members.as_slice()))
            .collect(),
        None => HashMap::new(),
    };
    let default_channels = match &config.default_channels {
        Some(channels) => channels.iter().map(String::as_str).collect(),
        None => DEFAULT_CHANNELS.to_vec(),
    };
    let ctx = ResolveContext {
        channel_alias,
        custom_channels,
        custom_multichannels,
        default_channels,
    };
    let channel_entries = config
        .channels
        .as_deref()
        .filter(|entries| !entries.is_empty())
        .map_or_else(
            || vec!["defaults"],
            |entries| entries.iter().map(String::as_str).collect(),
        );
    let channels = resolve_role(&channel_entries, &ctx)?;
    let allowlist_entries = config
        .allowlist_channels
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let allowlist = resolve_role(&allowlist_entries, &ctx)?;
    let denylist_entries = config
        .denylist_channels
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let denylist = resolve_role(&denylist_entries, &ctx)?;
    let channels = apply_allow_deny(channels, &allowlist, &denylist);

    Ok(ResolvedChannels {
        channels,
        channel_priority: config.channel_priority.unwrap_or(ChannelPriority::Flexible),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn empty_context(custom_multichannels: &BTreeMap<String, Vec<String>>) -> ResolveContext<'_> {
        ResolveContext {
            channel_alias: "https://conda.example.org",
            custom_channels: HashMap::new(),
            custom_multichannels: custom_multichannels
                .iter()
                .map(|(name, members)| (name.as_str(), members.as_slice()))
                .collect(),
            default_channels: Vec::new(),
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_string()).collect()
    }

    #[test]
    fn apply_allow_deny_with_empty_allowlist_applies_only_denylist() {
        // Given
        let channels = strings(&["alpha", "beta", "gamma"]);
        let denylist = strings(&["beta"]);

        // When
        let filtered = apply_allow_deny(channels, &[], &denylist);

        // Then
        assert_eq!(filtered, strings(&["alpha", "gamma"]));
    }

    #[test]
    fn apply_allow_deny_with_both_lists_applies_deny_then_allow() {
        // Given
        let channels = strings(&["alpha", "beta", "gamma", "delta"]);
        let allowlist = strings(&["alpha", "beta", "gamma"]);
        let denylist = strings(&["beta"]);

        // When
        let filtered = apply_allow_deny(channels, &allowlist, &denylist);

        // Then
        assert_eq!(filtered, strings(&["alpha", "gamma"]));
    }

    #[test]
    fn apply_allow_deny_removes_channel_present_in_both_lists() {
        // Given
        let channels = strings(&["alpha", "beta"]);
        let allowlist = strings(&["alpha", "beta"]);
        let denylist = strings(&["beta"]);

        // When
        let filtered = apply_allow_deny(channels, &allowlist, &denylist);

        // Then
        assert_eq!(filtered, strings(&["alpha"]));
    }

    #[test]
    fn apply_allow_deny_preserves_survivor_order_and_duplicates() {
        // Given
        let channels = strings(&["gamma", "shared", "alpha", "shared", "beta"]);
        let allowlist = strings(&["shared", "beta", "gamma"]);
        let denylist = strings(&["alpha"]);

        // When
        let filtered = apply_allow_deny(channels, &allowlist, &denylist);

        // Then
        assert_eq!(filtered, strings(&["gamma", "shared", "shared", "beta"]));
    }

    #[test]
    fn match_custom_channel_exact_match_joins_full_entry() {
        let custom_channels = HashMap::from([("acme", "https://internal.example.com/")]);

        let resolved = match_custom_channel("acme", &custom_channels);

        assert_eq!(
            resolved.as_deref(),
            Some("https://internal.example.com/acme")
        );
    }

    #[test]
    fn match_custom_channel_prefix_match_joins_original_full_entry() {
        let custom_channels = HashMap::from([("acme", "https://internal.example.com")]);

        let resolved = match_custom_channel("acme/label/dev", &custom_channels);

        assert_eq!(
            resolved.as_deref(),
            Some("https://internal.example.com/acme/label/dev")
        );
    }

    #[test]
    fn match_custom_channel_without_match_returns_none() {
        let custom_channels = HashMap::from([("acme", "https://internal.example.com")]);

        let resolved = match_custom_channel("other/label", &custom_channels);

        assert_eq!(resolved, None);
    }

    #[test]
    fn resolve_member_with_scheme_returns_entry_unchanged() {
        let resolved = resolve_member("file://local/channel", "https://conda.example.org");

        assert_eq!(resolved.as_deref(), Ok("file://local/channel"));
    }

    #[test]
    fn resolve_member_without_scheme_joins_alias_with_one_slash() {
        let resolved = resolve_member("acme", "https://conda.example.org/");

        assert_eq!(resolved.as_deref(), Ok("https://conda.example.org/acme"));
    }

    #[test]
    fn resolve_member_treats_named_entries_as_ordinary_bare_names() {
        let resolved = resolve_member("another-multichannel", "https://conda.example.org");

        assert_eq!(
            resolved.as_deref(),
            Ok("https://conda.example.org/another-multichannel")
        );
    }

    #[test]
    fn resolve_member_with_empty_alias_returns_entry_specific_error() {
        let resolved = resolve_member("acme", "");

        assert_eq!(
            resolved,
            Err(ExpandChannelsError::EmptyChannelAlias {
                entry: "acme".to_string(),
            })
        );
    }

    #[test]
    fn resolve_entry_with_scheme_returns_single_unchanged_entry() {
        let custom_multichannels = BTreeMap::new();
        let ctx = empty_context(&custom_multichannels);

        let resolved = resolve_entry("https://repo.example.org/channel", &ctx);

        assert_eq!(
            resolved,
            Ok(vec!["https://repo.example.org/channel".to_string()])
        );
    }

    #[test]
    fn resolve_entry_expands_custom_multichannel_members_in_order() {
        let custom_multichannels = BTreeMap::from([(
            "team".to_string(),
            vec![
                "https://repo.example.org/main".to_string(),
                "community".to_string(),
            ],
        )]);
        let ctx = empty_context(&custom_multichannels);

        let resolved = resolve_entry("team", &ctx);

        assert_eq!(
            resolved,
            Ok(vec![
                "https://repo.example.org/main".to_string(),
                "https://conda.example.org/community".to_string(),
            ])
        );
    }

    #[test]
    fn resolve_entry_prefers_custom_defaults_multichannel() {
        let custom_multichannels = BTreeMap::from([(
            "defaults".to_string(),
            vec!["https://custom.example.org/default".to_string()],
        )]);
        let mut ctx = empty_context(&custom_multichannels);
        ctx.default_channels = vec!["https://fallback.example.org/main"];

        let resolved = resolve_entry("defaults", &ctx);

        assert_eq!(
            resolved,
            Ok(vec!["https://custom.example.org/default".to_string()])
        );
    }

    #[test]
    fn resolve_entry_uses_default_channels_when_custom_defaults_is_absent() {
        let custom_multichannels = BTreeMap::new();
        let mut ctx = empty_context(&custom_multichannels);
        ctx.default_channels = vec!["https://repo.example.org/main", "community"];

        let resolved = resolve_entry("defaults", &ctx);

        assert_eq!(
            resolved,
            Ok(vec![
                "https://repo.example.org/main".to_string(),
                "https://conda.example.org/community".to_string(),
            ])
        );
    }

    #[test]
    fn resolve_entry_uses_custom_channel_before_alias() {
        let custom_multichannels = BTreeMap::new();
        let mut ctx = empty_context(&custom_multichannels);
        ctx.custom_channels = HashMap::from([("acme", "https://internal.example.com")]);

        let resolved = resolve_entry("acme/label", &ctx);

        assert_eq!(
            resolved,
            Ok(vec!["https://internal.example.com/acme/label".to_string()])
        );
    }

    #[test]
    fn resolve_entry_falls_back_to_channel_alias() {
        let custom_multichannels = BTreeMap::new();
        let ctx = empty_context(&custom_multichannels);

        let resolved = resolve_entry("community", &ctx);

        assert_eq!(
            resolved,
            Ok(vec!["https://conda.example.org/community".to_string()])
        );
    }

    #[test]
    fn resolve_entry_propagates_empty_alias_error_from_fallback() {
        let custom_multichannels = BTreeMap::new();
        let mut ctx = empty_context(&custom_multichannels);
        ctx.channel_alias = "";

        let resolved = resolve_entry("community", &ctx);

        assert_eq!(
            resolved,
            Err(ExpandChannelsError::EmptyChannelAlias {
                entry: "community".to_string(),
            })
        );
    }

    #[test]
    fn expand_channels_resolves_allowlist_and_denylist_entries_through_custom_channels() {
        let config = Config {
            channels: Some(strings(&["acme", "other", "extra"])),
            channel_alias: Some("https://conda.example.org".to_string()),
            custom_channels: Some(BTreeMap::from([(
                "acme".to_string(),
                "https://internal.example.com".to_string(),
            )])),
            allowlist_channels: Some(strings(&["acme", "other"])),
            denylist_channels: Some(strings(&["other"])),
            ..Config::default()
        };

        let resolved = expand_channels(&config).unwrap();

        assert_eq!(
            resolved.channels,
            strings(&["https://internal.example.com/acme"])
        );
    }
}
