/// Removes URL userinfo, `/t/<token>/` path segments, queries, and fragments.
pub fn redact_channel_url(value: &str) -> String {
    let value = value
        .find(['?', '#'])
        .map_or(value, |credential_start| &value[..credential_start]);
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

#[cfg(test)]
mod tests {
    use super::redact_channel_url;

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
    fn redact_channel_url_removes_query_and_fragment_credentials() {
        // Given
        let url = "https://repo.example/conda-forge?token=query-token#token=fragment-token";

        // When
        let redacted = redact_channel_url(url);

        // Then
        assert_eq!(redacted, "https://repo.example/conda-forge");
    }

    #[test]
    fn resolved_channels_from_channels_defaults_policy() {
        let config = condarc::ResolvedChannels::from_channels(vec!["conda-forge".to_string()]);

        assert_eq!(config.channel_priority, condarc::ChannelPriority::Strict);
        assert_eq!(config.channels, vec!["conda-forge"]);
    }
}
