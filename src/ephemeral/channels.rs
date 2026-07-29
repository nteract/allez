//! `ChannelConfig`/`ChannelSpec`/`ChannelPriorityMode`, the empty-`channels`
//! fallback (FR-015), and the allow/deny filter.

use std::fmt;

/// Fully resolved channel configuration for an environment request.
#[derive(Clone, PartialEq, Eq)]
pub struct ChannelConfig {
    /// Ordered channel references, highest priority first.
    pub channels: Vec<ChannelSpec>,
    /// The configured channel-priority policy.
    pub channel_priority: ChannelPriorityMode,
    /// Allowed channel references, or no allowlist when empty.
    pub allowed_channels: Vec<String>,
    /// Channel references that are always excluded.
    pub denied_channels: Vec<String>,
}

impl fmt::Debug for ChannelConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let allowed_channels = self
            .allowed_channels
            .iter()
            .map(|channel| redact_channel_url(channel))
            .collect::<Vec<_>>();
        let denied_channels = self
            .denied_channels
            .iter()
            .map(|channel| redact_channel_url(channel))
            .collect::<Vec<_>>();
        formatter
            .debug_struct("ChannelConfig")
            .field("channels", &self.channels)
            .field("channel_priority", &self.channel_priority)
            .field("allowed_channels", &allowed_channels)
            .field("denied_channels", &denied_channels)
            .finish()
    }
}

impl ChannelConfig {
    /// Creates unrestricted strict-priority configuration from channel references.
    pub fn from_urls(urls: Vec<String>) -> Self {
        Self {
            channels: urls
                .into_iter()
                .map(|url_or_name| ChannelSpec { url_or_name })
                .collect(),
            channel_priority: ChannelPriorityMode::Strict,
            allowed_channels: Vec::new(),
            denied_channels: Vec::new(),
        }
    }
}

/// One resolved channel name or URL.
#[derive(Clone, PartialEq, Eq)]
pub struct ChannelSpec {
    /// The opaque resolved channel name or URL.
    pub url_or_name: String,
}

impl fmt::Debug for ChannelSpec {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChannelSpec")
            .field("url_or_name", &redact_channel_url(&self.url_or_name))
            .finish()
    }
}

/// Channel priority as configured by conda.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPriorityMode {
    /// Prefer packages from the highest-priority matching channel.
    Strict,
    /// Permit lower-priority channels to win on package version.
    Flexible,
    /// Do not prioritize channel order.
    Disabled,
}

/// Removes URL userinfo and `/t/<token>/` conda token path segments.
pub fn redact_channel_url(value: &str) -> String {
    let (authority_start, scheme_less) = value
        .find("://")
        .map_or((0, true), |scheme_end| (scheme_end + 3, false));
    let authority_end = value[authority_start..]
        .find('/')
        .map_or(value.len(), |offset| authority_start + offset);
    let authority = &value[authority_start..authority_end];
    let without_userinfo = authority
        .rfind('@')
        .filter(|userinfo_end| !scheme_less || authority[..*userinfo_end].contains(':'))
        .map_or_else(
            || value.to_string(),
            |userinfo_end| {
                format!(
                    "{}{}",
                    &value[..authority_start],
                    &value[authority_start + userinfo_end + 1..]
                )
            },
        );

    let mut components = without_userinfo.split('/').peekable();
    let mut redacted = Vec::new();
    while let Some(component) = components.next() {
        if component == "t" && components.peek().is_some() {
            let _token = components.next();
            continue;
        }
        redacted.push(component);
    }
    redacted.join("/")
}

/// Substitutes the built-in defaults channel when no channel was configured.
pub fn channels_with_fallback(channels: &[ChannelSpec]) -> Vec<ChannelSpec> {
    if channels.is_empty() {
        vec![ChannelSpec {
            url_or_name: DEFAULTS_CHANNEL_NAME.to_string(),
        }]
    } else {
        channels.to_vec()
    }
}

/// The literal channel identifier FR-015's empty-`channels` fallback
/// substitutes; also the name allow/deny filtering matches against
/// (`denied_channels: vec!["defaults".to_string()]` still works after this
/// substitution runs).
pub(crate) const DEFAULTS_CHANNEL_NAME: &str = "defaults";

/// The real, network-resolvable URL backing [`DEFAULTS_CHANNEL_NAME`].
/// `rattler`'s own channel-alias resolution does not expand the bare name
/// `"defaults"` the way the conda CLI does (confirmed empirically: it maps
/// to `https://conda.anaconda.org/defaults`, which 404s) -- so this feature
/// substitutes a fixed, known-working URL, the same category of decision as
/// [`super::defaults::DEFAULT_PACKAGES`], not a `.condarc`-derived value.
pub(crate) const DEFAULTS_CHANNEL_URL: &str = "https://repo.anaconda.com/pkgs/main";

/// Returns the URL `rattler` should actually query for one channel: every
/// identifier is passed through unchanged except the literal
/// [`DEFAULTS_CHANNEL_NAME`], which resolves to [`DEFAULTS_CHANNEL_URL`] --
/// see that constant's own doc comment for why the bare name can't be
/// handed to `rattler` directly. Keeping `spec.url_or_name` itself as the
/// literal name (rather than substituting the URL into it directly) is what
/// lets [`filter_channels`]'s allow/deny matching still work against the
/// name `"defaults"` a caller configured.
pub(crate) fn resolve_channel_source(spec: &ChannelSpec) -> &str {
    if spec.url_or_name == DEFAULTS_CHANNEL_NAME {
        DEFAULTS_CHANNEL_URL
    } else {
        &spec.url_or_name
    }
}

/// Filters configured channels without changing the order of survivors,
/// per the configured deny-then-allow policy.
pub fn filter_channels(config: &ChannelConfig) -> Vec<ChannelSpec> {
    config
        .channels
        .iter()
        .filter(|channel| !config.denied_channels.contains(&channel.url_or_name))
        .filter(|channel| {
            config.allowed_channels.is_empty()
                || config.allowed_channels.contains(&channel.url_or_name)
        })
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        ChannelConfig, ChannelPriorityMode, ChannelSpec, channels_with_fallback, filter_channels,
        redact_channel_url,
    };

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
    fn channel_config_from_urls_defaults_policy() {
        let config = ChannelConfig::from_urls(vec!["conda-forge".to_string()]);

        assert_eq!(config.channel_priority, ChannelPriorityMode::Strict);
        assert!(config.allowed_channels.is_empty());
        assert!(config.denied_channels.is_empty());
        assert_eq!(config.channels[0].url_or_name, "conda-forge");
    }

    #[test]
    fn channel_spec_debug_redacts_credentials() {
        let channel = ChannelSpec {
            url_or_name: "https://user:password@repo.example/t/token-123/channel".to_string(),
        };

        let message = format!("{channel:?}");

        assert!(!message.contains("user:password"));
        assert!(!message.contains("token-123"));
        assert!(message.contains("https://repo.example/channel"));
    }

    #[test]
    fn channel_config_debug_redacts_credentials_in_all_channel_fields() {
        let config = ChannelConfig {
            channels: vec![ChannelSpec {
                url_or_name: "https://user:password@repo.example/t/token-123/channel".to_string(),
            }],
            channel_priority: ChannelPriorityMode::Strict,
            allowed_channels: vec![
                "https://allowed:secret@repo.example/t/token-456/channel".to_string(),
            ],
            denied_channels: vec![
                "https://denied:secret@repo.example/t/token-789/channel".to_string(),
            ],
        };

        let message = format!("{config:?}");

        assert!(!message.contains("user:password"));
        assert!(!message.contains("allowed:secret"));
        assert!(!message.contains("denied:secret"));
        assert!(!message.contains("token-123"));
        assert!(!message.contains("token-456"));
        assert!(!message.contains("token-789"));
    }

    #[test]
    fn channel_fallback_substitutes_defaults_only_when_empty() {
        assert_eq!(
            channels_with_fallback(&[]),
            vec![ChannelSpec {
                url_or_name: "defaults".to_string(),
            }]
        );

        let configured = vec![ChannelSpec {
            url_or_name: "conda-forge".to_string(),
        }];
        assert_eq!(channels_with_fallback(&configured), configured);
    }

    #[test]
    fn filter_channels_denies_before_applying_allowlist() {
        let config = ChannelConfig {
            channels: vec![
                ChannelSpec {
                    url_or_name: "first".to_string(),
                },
                ChannelSpec {
                    url_or_name: "blocked".to_string(),
                },
                ChannelSpec {
                    url_or_name: "allowed".to_string(),
                },
            ],
            channel_priority: ChannelPriorityMode::Strict,
            allowed_channels: vec!["blocked".to_string(), "allowed".to_string()],
            denied_channels: vec!["blocked".to_string()],
        };

        assert_eq!(
            filter_channels(&config),
            vec![ChannelSpec {
                url_or_name: "allowed".to_string(),
            }]
        );
    }

    #[test]
    fn filter_channels_without_allowlist_only_applies_denials() {
        let config = ChannelConfig {
            channels: vec![
                ChannelSpec {
                    url_or_name: "first".to_string(),
                },
                ChannelSpec {
                    url_or_name: "blocked".to_string(),
                },
                ChannelSpec {
                    url_or_name: "last".to_string(),
                },
            ],
            channel_priority: ChannelPriorityMode::Strict,
            allowed_channels: Vec::new(),
            denied_channels: vec!["blocked".to_string()],
        };

        assert_eq!(
            filter_channels(&config),
            vec![
                ChannelSpec {
                    url_or_name: "first".to_string(),
                },
                ChannelSpec {
                    url_or_name: "last".to_string(),
                },
            ]
        );
    }
}
