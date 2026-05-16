use crate::{AgentCoreError, AgentMessage, ContentBlock, Result, ToolSpec};
use std::collections::{BTreeMap, BTreeSet};

const MAX_PROVIDER_TOOL_NAME_LEN: usize = 64;

#[derive(Clone, Debug, Default)]
pub(crate) struct ProviderToolNameCodec {
    canonical_to_provider: BTreeMap<String, String>,
    provider_to_canonical: BTreeMap<String, String>,
}

impl ProviderToolNameCodec {
    #[cfg(test)]
    pub(crate) fn new(tools: &[ToolSpec]) -> Self {
        Self::new_with_extra_names(tools, std::iter::empty::<&str>())
    }

    pub(crate) fn new_with_message_history(tools: &[ToolSpec], messages: &[AgentMessage]) -> Self {
        Self::new_with_extra_names(tools, message_tool_names(messages))
    }

    fn new_with_extra_names<'a>(
        tools: &[ToolSpec],
        extra_names: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for tool in tools {
            groups
                .entry(provider_safe_base(&tool.name))
                .or_default()
                .insert(tool.name.clone());
        }
        for name in extra_names {
            groups
                .entry(provider_safe_base(name))
                .or_default()
                .insert(name.to_owned());
        }

        let mut used = BTreeSet::new();
        let mut canonical_to_provider = BTreeMap::new();
        let mut provider_to_canonical = BTreeMap::new();
        for (base, canonical_names) in groups {
            let needs_suffix = canonical_names.len() > 1 || used.contains(&base);
            for canonical in canonical_names {
                let provider = unique_provider_name(&base, &canonical, needs_suffix, &mut used);
                canonical_to_provider.insert(canonical.clone(), provider.clone());
                provider_to_canonical.insert(provider, canonical);
            }
        }

        Self {
            canonical_to_provider,
            provider_to_canonical,
        }
    }

    pub(crate) fn provider_name(
        &self,
        canonical: &str,
        provider_id: &str,
        model: &str,
    ) -> Result<String> {
        self.canonical_to_provider
            .get(canonical)
            .cloned()
            .ok_or_else(|| {
                AgentCoreError::Provider(format!(
                    "provider tool-name encode failed for provider `{provider_id}` model `{model}`: undeclared canonical tool `{canonical}`; declared canonical tools: {}",
                    display_list(self.canonical_to_provider.keys()),
                ))
            })
    }

    pub(crate) fn canonical_name(
        &self,
        provider: &str,
        provider_id: &str,
        model: &str,
    ) -> Result<String> {
        self.provider_to_canonical
            .get(provider)
            .cloned()
            .ok_or_else(|| {
                AgentCoreError::Provider(format!(
                    "provider tool-name decode failed for provider `{provider_id}` model `{model}`: unknown provider tool alias `{provider}`; known aliases: {}",
                    display_aliases(&self.provider_to_canonical),
                ))
            })
    }
}

fn message_tool_names(messages: &[AgentMessage]) -> impl Iterator<Item = &str> {
    messages.iter().flat_map(|message| {
        message.content.iter().filter_map(|block| match block {
            ContentBlock::ToolCall { tool_call } => Some(tool_call.name.as_str()),
            ContentBlock::ToolResult { tool_name, .. } => Some(tool_name.as_str()),
            _ => None,
        })
    })
}

fn unique_provider_name(
    base: &str,
    canonical: &str,
    needs_suffix: bool,
    used: &mut BTreeSet<String>,
) -> String {
    let suffix = stable_suffix(canonical);
    let mut candidate = if needs_suffix || base.len() > MAX_PROVIDER_TOOL_NAME_LEN {
        suffixed_provider_name(base, &suffix, None)
    } else {
        base.into()
    };
    let mut attempt = 2_u32;
    while used.contains(&candidate) {
        candidate = suffixed_provider_name(base, &suffix, Some(attempt));
        attempt += 1;
    }
    used.insert(candidate.clone());
    candidate
}

fn provider_safe_base(canonical: &str) -> String {
    let mut encoded = canonical
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' || character == '-' {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();
    if encoded.is_empty() {
        encoded.push_str("tool");
    }
    encoded
}

fn suffixed_provider_name(base: &str, suffix: &str, attempt: Option<u32>) -> String {
    let suffix = match attempt {
        Some(attempt) => format!("_{suffix}_{attempt}"),
        None => format!("_{suffix}"),
    };
    let max_base_len = MAX_PROVIDER_TOOL_NAME_LEN.saturating_sub(suffix.len());
    let truncated = if max_base_len == 0 {
        String::new()
    } else {
        base.chars().take(max_base_len).collect::<String>()
    };
    format!("{truncated}{suffix}")
}

fn stable_suffix(value: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in value.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{:08x}", hash as u32)
}

fn display_list<'a>(values: impl Iterator<Item = &'a String>) -> String {
    let values = values.map(String::as_str).collect::<Vec<_>>();
    if values.is_empty() {
        "<none>".into()
    } else {
        values.join(", ")
    }
}

fn display_aliases(aliases: &BTreeMap<String, String>) -> String {
    if aliases.is_empty() {
        return "<none>".into();
    }
    aliases
        .iter()
        .map(|(provider, canonical)| format!("{provider}->{canonical}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::{MAX_PROVIDER_TOOL_NAME_LEN, ProviderToolNameCodec, stable_suffix};
    use crate::ToolSpec;
    use serde_json::json;

    #[test]
    fn valid_names_stay_readable() -> crate::Result<()> {
        let codec = ProviderToolNameCodec::new(&[tool("lookup")]);

        assert_eq!(
            codec.provider_name("lookup", "provider", "model")?,
            "lookup"
        );
        assert_eq!(
            codec.canonical_name("lookup", "provider", "model")?,
            "lookup"
        );
        Ok(())
    }

    #[test]
    fn dotted_names_round_trip_through_safe_aliases() -> crate::Result<()> {
        let codec = ProviderToolNameCodec::new(&[tool("host.exec.start")]);

        assert_eq!(
            codec.provider_name("host.exec.start", "provider", "model")?,
            "host_exec_start"
        );
        assert_eq!(
            codec.canonical_name("host_exec_start", "provider", "model")?,
            "host.exec.start"
        );
        Ok(())
    }

    #[test]
    fn colliding_aliases_get_stable_suffixes() -> crate::Result<()> {
        let codec = ProviderToolNameCodec::new(&[tool("host.exec.start"), tool("host_exec_start")]);
        let dotted = codec.provider_name("host.exec.start", "provider", "model")?;
        let underscored = codec.provider_name("host_exec_start", "provider", "model")?;

        assert_ne!(dotted, underscored);
        assert!(dotted.starts_with("host_exec_start_"));
        assert!(underscored.starts_with("host_exec_start_"));
        assert_eq!(
            codec.canonical_name(&dotted, "provider", "model")?,
            "host.exec.start"
        );
        assert_eq!(
            codec.canonical_name(&underscored, "provider", "model")?,
            "host_exec_start"
        );
        Ok(())
    }

    #[test]
    fn long_aliases_keep_suffix_inside_provider_limit() -> crate::Result<()> {
        let canonical = "tool.".to_string() + &"segment.".repeat(20);
        let codec = ProviderToolNameCodec::new(&[tool(&canonical)]);
        let provider = codec.provider_name(&canonical, "provider", "model")?;

        assert!(provider.len() <= MAX_PROVIDER_TOOL_NAME_LEN);
        assert!(provider.ends_with(&stable_suffix(&canonical)));
        assert_eq!(
            codec.canonical_name(&provider, "provider", "model")?,
            canonical
        );
        Ok(())
    }

    #[test]
    fn unknown_alias_reports_provider_context() {
        let codec = ProviderToolNameCodec::new(&[tool("host.exec.start")]);
        let error = codec
            .canonical_name("unknown_alias", "test-provider", "test-model")
            .expect_err("unknown alias should be rejected")
            .to_string();

        assert!(error.contains("unknown_alias"));
        assert!(error.contains("test-provider"));
        assert!(error.contains("test-model"));
        assert!(error.contains("host_exec_start->host.exec.start"));
    }

    fn tool(name: &str) -> ToolSpec {
        ToolSpec {
            name: name.into(),
            description: String::new(),
            input_schema: json!({ "type": "object" }),
            execution_mode: None,
            permissions: Vec::new(),
        }
    }
}
