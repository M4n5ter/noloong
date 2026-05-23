use super::{
    AnthropicProviderReasoningEffort, AnthropicProviderThinkingMode, BuiltInProviderConfig,
    ChatCompletionsReasoningEffort, ChatGptAuthConfig, ContextCompactionMode, HostProfileConfig,
    ProfileCompactionConfig, ProfileEventStoreConfig, ResponsesProviderReasoningEffort,
    ResponsesProviderReasoningSummary, ResponsesStateMode, RuntimeProfileConfig,
    ensure_sqlite_database_parent, resolve_chatgpt_token_file_with_env,
    resolve_profile_config_path_with_env, resolve_state_database_url_with_env,
    starter_profile_config,
};
use crate::test_support::{remove_temp_file, write_temp_file};
use std::path::PathBuf;

#[test]
fn profile_config_loads_chat_completions() {
    let config = serde_json::from_value::<HostProfileConfig>(serde_json::json!(
        {
            "profiles": [
                {
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "chat_completions",
                        "model": "gpt-5.4-mini",
                        "apiKeyEnv": "OPENROUTER_API_KEY"
                    }
                }
            ]
        }
    ))
    .unwrap();

    assert!(config.validate().is_ok());
    assert!(matches!(
        config.profiles[0].provider,
        BuiltInProviderConfig::ChatCompletions { .. }
    ));
    assert_eq!(config.profiles[0].event_store, None);
}

#[test]
fn profile_config_loads_chat_completions_reasoning() {
    let config = serde_json::from_value::<RuntimeProfileConfig>(serde_json::json!(
        {
            "profileId": "default",
            "displayName": "Default",
            "provider": {
                "type": "chat_completions",
                "model": "gpt-5.4-mini",
                "reasoning": {
                    "enabled": true,
                    "effort": "xhigh"
                }
            }
        }
    ))
    .unwrap();

    let BuiltInProviderConfig::ChatCompletions {
        reasoning: Some(reasoning),
        ..
    } = config.provider
    else {
        panic!("expected Chat Completions reasoning");
    };
    assert!(reasoning.enabled);
    assert_eq!(
        reasoning.effort,
        Some(ChatCompletionsReasoningEffort::XHigh)
    );
}

#[test]
fn profile_config_loads_responses_reasoning() {
    let config = serde_json::from_value::<RuntimeProfileConfig>(serde_json::json!(
        {
            "profileId": "default",
            "displayName": "Default",
            "provider": {
                "type": "responses",
                "model": "gpt-5.4-mini",
                "reasoning": {
                    "effort": "medium",
                    "summary": "detailed",
                    "includeEncrypted": true
                }
            }
        }
    ))
    .unwrap();

    let BuiltInProviderConfig::Responses {
        reasoning: Some(reasoning),
        ..
    } = config.provider
    else {
        panic!("expected Responses reasoning");
    };
    assert!(reasoning.enabled);
    assert_eq!(
        reasoning.effort,
        Some(ResponsesProviderReasoningEffort::Medium)
    );
    assert_eq!(
        reasoning.summary,
        Some(ResponsesProviderReasoningSummary::Detailed)
    );
    assert_eq!(reasoning.include_encrypted, Some(true));
}

#[test]
fn profile_config_loads_responses_state_mode() {
    let config = serde_json::from_value::<RuntimeProfileConfig>(serde_json::json!(
        {
            "profileId": "default",
            "displayName": "Default",
            "provider": {
                "type": "responses",
                "model": "gpt-5.4-mini",
                "stateMode": "stateful"
            }
        }
    ))
    .unwrap();

    let BuiltInProviderConfig::Responses { state_mode, .. } = config.provider else {
        panic!("expected Responses provider");
    };
    assert_eq!(state_mode, ResponsesStateMode::Stateful);
}

#[test]
fn profile_config_loads_responses_file_data_url_opt_in() {
    let config = serde_json::from_value::<RuntimeProfileConfig>(serde_json::json!(
        {
            "profileId": "default",
            "displayName": "Default",
            "provider": {
                "type": "responses",
                "model": "gpt-5.4-mini",
                "allowFileDataUrlInput": true
            }
        }
    ))
    .unwrap();

    let BuiltInProviderConfig::Responses {
        allow_file_data_url_input,
        ..
    } = config.provider
    else {
        panic!("expected Responses provider");
    };
    assert!(allow_file_data_url_input);
}

#[test]
fn profile_config_rejects_stateless_reasoning_without_encrypted_replay() {
    let config = serde_json::from_value::<HostProfileConfig>(serde_json::json!(
        {
            "profiles": [
                {
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "chatgpt_responses",
                        "model": "gpt-5.4-mini",
                        "stateMode": "stateless",
                        "reasoning": {
                            "enabled": true,
                            "includeEncrypted": false
                        }
                    }
                }
            ]
        }
    ))
    .unwrap();

    let error = config
        .validate()
        .expect_err("invalid stateless reasoning config");

    assert!(error.to_string().contains("includeEncrypted"));
}

#[test]
fn profile_config_loads_anthropic_reasoning() {
    let config = serde_json::from_value::<RuntimeProfileConfig>(serde_json::json!(
        {
            "profileId": "default",
            "displayName": "Default",
            "provider": {
                "type": "anthropic_messages",
                "model": "claude-opus-4-7",
                "reasoning": {
                    "effort": "max",
                    "thinking": "adaptive"
                }
            }
        }
    ))
    .unwrap();

    let BuiltInProviderConfig::AnthropicMessages {
        reasoning: Some(reasoning),
        ..
    } = config.provider
    else {
        panic!("expected Anthropic reasoning");
    };
    assert_eq!(
        reasoning.effort,
        Some(AnthropicProviderReasoningEffort::Max)
    );
    assert_eq!(
        reasoning.thinking,
        Some(AnthropicProviderThinkingMode::Adaptive)
    );
}

#[test]
fn profile_config_load_reads_json_file() {
    let path = write_temp_file(
        "profile-json",
        "json",
        r#"{
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "responses", "model": "gpt-5.4-mini"}
                }]
            }"#,
    );

    let config = HostProfileConfig::load(&path).unwrap();
    remove_temp_file(path);

    assert_eq!(config.profiles[0].profile_id, "default");
}

#[test]
fn profile_config_load_reads_jsonc_file() {
    let path = write_temp_file(
        "profile-jsonc",
        "jsonc",
        r#"{
                // Editor tooling can keep comments in profile configs.
                "profiles": [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "responses", "model": "gpt-5.4-mini"},
                }],
            }"#,
    );

    let config = HostProfileConfig::load(&path).unwrap();
    remove_temp_file(path);

    assert_eq!(config.profiles[0].profile_id, "default");
}

#[test]
fn profile_config_loads_jsonc_example() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/profile-configs/telegram-openrouter-free.jsonc");

    let config = HostProfileConfig::load(path).unwrap();

    config.validate().unwrap();
    assert_eq!(config.profiles[0].profile_id, "telegram-openrouter-free");
}

#[test]
fn profile_config_loads_weixin_chatgpt_example() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/profile-configs/weixin-chatgpt-subscription.json");

    let config = HostProfileConfig::load(path).unwrap();

    config.validate().unwrap();
    assert_eq!(config.default_profile_id.as_deref(), Some("weixin-chatgpt"));
    assert_eq!(config.profiles[0].metadata["channel"], "weixin");
}

#[test]
fn profile_config_load_rejects_json5_only_syntax() {
    let path = write_temp_file(
        "profile-json5",
        "jsonc",
        r#"{
                profiles: [{
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {"type": "responses", "model": "gpt-5.4-mini"}
                }]
            }"#,
    );

    let error = HostProfileConfig::load(&path).unwrap_err();
    remove_temp_file(path);

    assert!(error.to_string().contains("failed to parse profile config"));
}

#[test]
fn runtime_profile_config_loads_sqlite_event_store() {
    let config = serde_json::from_value::<RuntimeProfileConfig>(serde_json::json!(
        {
            "profileId": "default",
            "displayName": "Default",
            "provider": {
                "type": "responses",
                "model": "gpt-5.4-mini"
            },
            "eventStore": {
                "type": "sqlite",
                "databaseUrl": "sqlite:target/noloong-events.sqlite"
            }
        }
    ))
    .unwrap();

    assert_eq!(
        config.event_store,
        Some(ProfileEventStoreConfig::Sqlite {
            database_url: "sqlite:target/noloong-events.sqlite".into(),
            migrate_on_connect: true,
        })
    );
}

#[test]
fn runtime_profile_config_loads_sqlite_event_store_without_migrations() {
    let config = serde_json::from_value::<RuntimeProfileConfig>(serde_json::json!(
        {
            "profileId": "default",
            "displayName": "Default",
            "provider": {
                "type": "responses",
                "model": "gpt-5.4-mini"
            },
            "eventStore": {
                "type": "sqlite",
                "databaseUrl": "sqlite:target/noloong-events.sqlite",
                "migrateOnConnect": false
            }
        }
    ))
    .unwrap();

    assert_eq!(
        config.event_store,
        Some(ProfileEventStoreConfig::Sqlite {
            database_url: "sqlite:target/noloong-events.sqlite".into(),
            migrate_on_connect: false,
        })
    );
}

#[test]
fn profile_config_rejects_unknown_provider() {
    let error = serde_json::from_value::<HostProfileConfig>(serde_json::json!(
        {
            "profiles": [
                {
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "unknown",
                        "model": "x"
                    }
                }
            ]
        }
    ))
    .unwrap_err();

    assert!(error.to_string().contains("unknown variant"));
}

#[test]
fn profile_config_rejects_duplicate_profile_ids() {
    let config = serde_json::from_value::<HostProfileConfig>(serde_json::json!(
        {
            "profiles": [
                {
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "responses",
                        "model": "gpt-5.4-mini"
                    }
                },
                {
                    "profileId": "default",
                    "displayName": "Duplicate",
                    "provider": {
                        "type": "responses",
                        "model": "gpt-5.4-mini"
                    }
                }
            ]
        }
    ))
    .unwrap();

    let error = config.validate().unwrap_err();

    assert!(matches!(
        error,
        super::CliConfigError::DuplicateProfileId(_)
    ));
}

#[test]
fn profile_config_builds_registry_store_config() {
    let config = serde_json::from_value::<HostProfileConfig>(serde_json::json!(
        {
            "registryStore": {
                "type": "sqlite",
                "databaseUrl": "sqlite::memory:"
            },
            "profiles": [
                {
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "responses",
                        "model": "gpt-5.4-mini"
                    }
                }
            ]
        }
    ))
    .unwrap();

    assert!(config.validate().is_ok());
    assert!(matches!(
        config.registry_store,
        Some(super::RegistryStoreConfig::Sqlite { .. })
    ));
}

#[test]
fn state_database_url_uses_default_home_path() {
    let url = resolve_state_database_url_with_env(|name| match name {
        "HOME" => Some("/home/alice".into()),
        _ => None,
    })
    .unwrap();

    assert_eq!(url, "sqlite:/home/alice/.agents/noloong/state.sqlite");
}

#[test]
fn state_database_url_prefers_env() {
    let url = resolve_state_database_url_with_env(|name| match name {
        "HOME" => Some("/home/alice".into()),
        "NOLOONG_STATE_DATABASE_URL" => Some("sqlite:/tmp/noloong.sqlite".into()),
        _ => None,
    })
    .unwrap();

    assert_eq!(url, "sqlite:/tmp/noloong.sqlite");
}

#[test]
fn state_database_url_ignores_empty_env() {
    let url = resolve_state_database_url_with_env(|name| match name {
        "HOME" => Some("/home/alice".into()),
        "NOLOONG_STATE_DATABASE_URL" => Some("   ".into()),
        _ => None,
    })
    .unwrap();

    assert_eq!(url, "sqlite:/home/alice/.agents/noloong/state.sqlite");
}

#[test]
fn state_database_parent_accepts_supported_sqlite_urls() {
    ensure_sqlite_database_parent("sqlite::memory:").unwrap();
    assert!(ensure_sqlite_database_parent("postgres://localhost/db").is_err());
}

#[test]
fn sqlite_database_parent_is_created() {
    let dir = crate::test_support::temp_dir("state-database-parent");
    let db = dir.join("nested").join("state.sqlite");
    let url = format!("sqlite:{}", db.display());

    ensure_sqlite_database_parent(&url).unwrap();

    assert!(db.parent().unwrap().is_dir());
    crate::test_support::remove_temp_dir(dir);
}

#[test]
fn profile_config_loads_default_plugins() {
    let config = serde_json::from_value::<HostProfileConfig>(serde_json::json!(
        {
            "profiles": [
                {
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "responses",
                        "model": "gpt-5.4-mini"
                    },
                    "plugins": [
                        {
                            "pluginId": "echo",
                            "displayName": "Echo",
                            "enabled": true,
                            "components": [
                                {
                                    "type": "noloong_extension",
                                    "transport": {
                                        "type": "stdio",
                                        "command": "node",
                                        "args": [
                                            "examples/extensions/echo.mjs"
                                        ],
                                        "env": {
                                            "PATH": {
                                                "type": "host_env",
                                                "name": "PATH"
                                            }
                                        }
                                    },
                                    "allowedCapabilities": [
                                        {
                                            "type": "tool",
                                            "name": "echo.run"
                                        }
                                    ]
                                }
                            ]
                        }
                    ]
                }
            ]
        }
    ))
    .unwrap();

    config.validate().unwrap();
    assert_eq!(config.profiles[0].plugins.len(), 1);
    assert_eq!(config.profiles[0].plugins[0].plugin_id, "echo");
}

#[test]
fn profile_config_loads_chatgpt_responses_with_default_token_file_auth() {
    let config = serde_json::from_value::<HostProfileConfig>(serde_json::json!(
        {
            "profiles": [
                {
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "chatgpt_responses",
                        "model": "gpt-5.4-mini"
                    }
                }
            ]
        }
    ))
    .unwrap();

    let BuiltInProviderConfig::ChatgptResponses { auth, .. } = &config.profiles[0].provider else {
        panic!("expected ChatGPT responses provider");
    };
    assert_eq!(auth, &ChatGptAuthConfig::default());
    assert_eq!(config.profiles[0].compaction, ProfileCompactionConfig::Auto);
}

#[test]
fn profile_config_loads_chatgpt_responses_file_data_url_opt_in() {
    let config = serde_json::from_value::<HostProfileConfig>(serde_json::json!(
        {
            "profiles": [
                {
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "chatgpt_responses",
                        "model": "gpt-5.4-mini",
                        "allowFileDataUrlInput": true
                    }
                }
            ]
        }
    ))
    .unwrap();

    let BuiltInProviderConfig::ChatgptResponses {
        allow_file_data_url_input,
        ..
    } = &config.profiles[0].provider
    else {
        panic!("expected ChatGPT responses provider");
    };
    assert!(*allow_file_data_url_input);
}

#[test]
fn profile_config_loads_chatgpt_env_headers_escape_hatch() {
    let config = serde_json::from_value::<HostProfileConfig>(serde_json::json!(
        {
            "profiles": [
                {
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "chatgpt_responses",
                        "model": "gpt-5.4-mini",
                        "auth": {
                            "type": "env_headers",
                            "id": "custom-auth",
                            "headers": [
                                {
                                    "name": "Authorization",
                                    "env": "CHATGPT_ACCESS_TOKEN",
                                    "valuePrefix": "Bearer "
                                }
                            ]
                        }
                    }
                }
            ]
        }
    ))
    .unwrap();

    let BuiltInProviderConfig::ChatgptResponses { auth, .. } = &config.profiles[0].provider else {
        panic!("expected ChatGPT responses provider");
    };
    assert!(matches!(auth, ChatGptAuthConfig::EnvHeaders { .. }));
}

#[test]
fn profile_config_loads_openai_responses_compaction() {
    let config = serde_json::from_value::<HostProfileConfig>(serde_json::json!(
        {
            "profiles": [
                {
                    "profileId": "default",
                    "displayName": "Default",
                    "provider": {
                        "type": "chatgpt_responses",
                        "model": "gpt-5.4-mini"
                    },
                    "compaction": {
                        "type": "openai_responses",
                        "inputLimitModel": "gpt-5.4-mini",
                        "compactModel": "gpt-5.4-mini",
                        "inputLimitTokens": 200000,
                        "triggerRatio": 0.8,
                        "summaryBudgetTokens": 32000,
                        "keepRecentTokens": 64000,
                        "mode": "request_only",
                        "requestTimeoutSecs": 120
                    }
                }
            ]
        }
    ))
    .unwrap();

    let ProfileCompactionConfig::OpenaiResponses {
        input_limit_model,
        compact_model,
        input_limit_tokens,
        trigger_ratio,
        summary_budget_tokens,
        keep_recent_tokens,
        mode,
        request_timeout_secs,
        ..
    } = &config.profiles[0].compaction
    else {
        panic!("expected OpenAI responses compaction");
    };
    assert_eq!(input_limit_model.as_deref(), Some("gpt-5.4-mini"));
    assert_eq!(compact_model.as_deref(), Some("gpt-5.4-mini"));
    assert_eq!(*input_limit_tokens, Some(200_000));
    assert_eq!(*trigger_ratio, Some(0.8));
    assert_eq!(*summary_budget_tokens, Some(32_000));
    assert_eq!(*keep_recent_tokens, Some(64_000));
    assert_eq!(*mode, Some(ContextCompactionMode::RequestOnly));
    assert_eq!(*request_timeout_secs, Some(120));
}

#[test]
fn chatgpt_token_file_resolver_uses_default_home_path() {
    let path = resolve_chatgpt_token_file_with_env(None, None, |name| match name {
        "HOME" => Some("/home/alice".into()),
        _ => None,
    })
    .unwrap();

    assert_eq!(
        path,
        PathBuf::from("/home/alice/.agents/noloong/chatgpt/token.json")
    );
}

#[test]
fn profile_config_path_resolver_prefers_explicit_path() {
    let path = resolve_profile_config_path_with_env(Some("~/custom.jsonc"), |name| match name {
        "HOME" => Some("/home/alice".into()),
        "NOLOONG_PROFILE_CONFIG" => Some("/ignored/profile.jsonc".into()),
        _ => None,
    })
    .unwrap();

    assert_eq!(path, PathBuf::from("/home/alice/custom.jsonc"));
}

#[test]
fn profile_config_path_resolver_uses_env_before_default() {
    let path = resolve_profile_config_path_with_env(None, |name| match name {
        "HOME" => Some("/home/alice".into()),
        "NOLOONG_PROFILE_CONFIG" => Some("~/configured.jsonc".into()),
        _ => None,
    })
    .unwrap();

    assert_eq!(path, PathBuf::from("/home/alice/configured.jsonc"));
}

#[test]
fn profile_config_path_resolver_uses_default_home_path() {
    let path = resolve_profile_config_path_with_env(None, |name| match name {
        "HOME" => Some("/home/alice".into()),
        _ => None,
    })
    .unwrap();

    assert_eq!(
        path,
        PathBuf::from("/home/alice/.agents/noloong/profile-config.jsonc")
    );
}

#[test]
fn starter_profile_config_is_valid_and_canonical_serializable() {
    let config = starter_profile_config();

    config.validate().unwrap();
    let text = config.to_canonical_json().unwrap();
    let reloaded: HostProfileConfig = serde_json::from_str(&text).unwrap();

    assert_eq!(
        reloaded.default_profile_id.as_deref(),
        Some("chatgpt-responses")
    );
    assert!(matches!(
        reloaded.profiles[0].provider,
        BuiltInProviderConfig::ChatgptResponses { .. }
    ));
}

#[test]
fn chatgpt_token_file_resolver_prefers_explicit_path() {
    let path = resolve_chatgpt_token_file_with_env(
        Some("~/custom-token.json"),
        Some("CUSTOM_TOKEN"),
        |name| match name {
            "HOME" => Some("/home/alice".into()),
            "CUSTOM_TOKEN" => Some("/ignored/token.json".into()),
            "NOLOONG_CHATGPT_TOKEN_FILE" => Some("/ignored/default-token.json".into()),
            _ => None,
        },
    )
    .unwrap();

    assert_eq!(path, PathBuf::from("/home/alice/custom-token.json"));
}

#[test]
fn chatgpt_token_file_resolver_uses_named_env_before_default_env() {
    let path = resolve_chatgpt_token_file_with_env(None, Some("CUSTOM_TOKEN"), |name| match name {
        "HOME" => Some("/home/alice".into()),
        "CUSTOM_TOKEN" => Some("~/from-custom-env.json".into()),
        "NOLOONG_CHATGPT_TOKEN_FILE" => Some("/ignored/default-token.json".into()),
        _ => None,
    })
    .unwrap();

    assert_eq!(path, PathBuf::from("/home/alice/from-custom-env.json"));
}

#[test]
fn chatgpt_token_file_resolver_uses_default_env_before_home_default() {
    let path = resolve_chatgpt_token_file_with_env(None, None, |name| match name {
        "HOME" => Some("/home/alice".into()),
        "NOLOONG_CHATGPT_TOKEN_FILE" => Some("/tmp/token.json".into()),
        _ => None,
    })
    .unwrap();

    assert_eq!(path, PathBuf::from("/tmp/token.json"));
}
