use super::{Cli, CliCommand, CliError, validate_interaction_bind};
use crate::build_info_cli::{BuildInfoSourceSubcommand, BuildInfoSubcommand};
use crate::cli::profile_locale;
use crate::config::HostProfileConfig;
use crate::profile_config_cli::{
    ProfileConfigSchemaOptions, ProfileConfigSubcommand, run_profile_config_schema,
};
use crate::schema::profile_config_schema_json;
use crate::test_support::{remove_temp_file, write_temp_file};
use crate::weixin_cli::{WeixinBridgeOptions, WeixinSubcommand, weixin_config_from_values};
use clap::Parser;
use noloong_agent::Locale;
use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf};

#[test]
fn cli_serve_rejects_public_bind_without_token() {
    let bind: SocketAddr = "0.0.0.0:8787".parse().unwrap();

    let error = validate_interaction_bind(bind, None).unwrap_err();

    assert!(matches!(error, CliError::PublicBindWithoutToken(_)));
}

#[test]
fn cli_telegram_embeds_loopback_interaction_options() {
    let cli = Cli::try_parse_from([
        "noloong",
        "telegram",
        "--profile-config",
        "profiles.json",
        "--telegram-bot-username",
        "noloong_bot",
        "--telegram-allowed-users",
        "123456789",
        "--telegram-locale",
        "zh",
    ])
    .unwrap();

    let CliCommand::Telegram(options) = cli.command else {
        panic!("expected telegram command");
    };
    assert_eq!(options.profile_config.as_deref(), Some("profiles.json"));
    assert_eq!(options.bridge.bot_username.as_deref(), Some("noloong_bot"));
    assert_eq!(options.bridge.allowed_users.as_deref(), Some("123456789"));
    assert_eq!(options.bridge.locale, Some(Locale::Zh));
}

#[test]
fn cli_weixin_run_embeds_loopback_interaction_options() {
    let cli = Cli::try_parse_from([
        "noloong",
        "weixin",
        "run",
        "--profile-config",
        "profiles.json",
        "--weixin-account-id",
        "wx-bot",
        "--weixin-allowed-users",
        "user-1,user-2",
        "--weixin-locale",
        "zh",
    ])
    .unwrap();

    let CliCommand::Weixin(command) = cli.command else {
        panic!("expected weixin command");
    };
    let WeixinSubcommand::Run(options) = command.command else {
        panic!("expected weixin run");
    };
    assert_eq!(options.profile_config.as_deref(), Some("profiles.json"));
    assert_eq!(options.bridge.account_id.as_deref(), Some("wx-bot"));
    assert_eq!(
        options.bridge.allowed_users.as_deref(),
        Some("user-1,user-2")
    );
    assert_eq!(options.bridge.locale, Some(Locale::Zh));
}

#[test]
fn cli_profile_config_schema_command_parses() {
    let cli = Cli::try_parse_from([
        "noloong",
        "profile-config",
        "schema",
        "--check",
        "schemas/profile-config.schema.json",
    ])
    .unwrap();

    let CliCommand::ProfileConfig(command) = cli.command else {
        panic!("expected profile-config command");
    };
    let ProfileConfigSubcommand::Schema(options) = command.command;
    assert_eq!(
        options.check,
        Some(PathBuf::from("schemas/profile-config.schema.json"))
    );
}

#[test]
fn cli_build_info_commands_parse() {
    let manifest = Cli::try_parse_from(["noloong", "build-info", "manifest"]).unwrap();
    let CliCommand::BuildInfo(command) = manifest.command else {
        panic!("expected build-info command");
    };
    assert!(matches!(command.command, BuildInfoSubcommand::Manifest));

    let list = Cli::try_parse_from(["noloong", "build-info", "source", "list"]).unwrap();
    let CliCommand::BuildInfo(command) = list.command else {
        panic!("expected build-info command");
    };
    let BuildInfoSubcommand::Source(source) = command.command else {
        panic!("expected build-info source command");
    };
    assert!(matches!(source.command, BuildInfoSourceSubcommand::List));

    let cat =
        Cli::try_parse_from(["noloong", "build-info", "source", "cat", "Cargo.toml"]).unwrap();
    let CliCommand::BuildInfo(command) = cat.command else {
        panic!("expected build-info command");
    };
    let BuildInfoSubcommand::Source(source) = command.command else {
        panic!("expected build-info source command");
    };
    let BuildInfoSourceSubcommand::Cat(options) = source.command else {
        panic!("expected build-info source cat command");
    };
    assert_eq!(options.path, "Cargo.toml");

    let extract = Cli::try_parse_from([
        "noloong",
        "build-info",
        "source",
        "extract",
        "--output-dir",
        "out",
        "--force",
    ])
    .unwrap();
    let CliCommand::BuildInfo(command) = extract.command else {
        panic!("expected build-info command");
    };
    let BuildInfoSubcommand::Source(source) = command.command else {
        panic!("expected build-info source command");
    };
    let BuildInfoSourceSubcommand::Extract(options) = source.command else {
        panic!("expected build-info source extract command");
    };
    assert_eq!(options.output_dir, PathBuf::from("out"));
    assert!(options.force);

    let archive = Cli::try_parse_from([
        "noloong",
        "build-info",
        "source",
        "archive",
        "--output",
        "source.tar.zst",
    ])
    .unwrap();
    let CliCommand::BuildInfo(command) = archive.command else {
        panic!("expected build-info command");
    };
    let BuildInfoSubcommand::Source(source) = command.command else {
        panic!("expected build-info source command");
    };
    let BuildInfoSourceSubcommand::Archive(options) = source.command else {
        panic!("expected build-info source archive command");
    };
    assert_eq!(options.output, PathBuf::from("source.tar.zst"));
}

#[test]
fn cli_profile_config_schema_rejects_output_and_check_together() {
    let error = Cli::try_parse_from([
        "noloong",
        "profile-config",
        "schema",
        "--output",
        "schemas/profile-config.schema.json",
        "--check",
        "schemas/profile-config.schema.json",
    ])
    .unwrap_err();

    assert!(error.to_string().contains("cannot be used with"));
}

#[test]
fn profile_config_schema_check_accepts_matching_file() {
    let path = write_temp_file("profile-schema", "json", &profile_config_schema_json());

    run_profile_config_schema(ProfileConfigSchemaOptions {
        check: Some(path.clone()),
        ..Default::default()
    })
    .unwrap();
    remove_temp_file(path);
}

#[test]
fn profile_config_schema_check_rejects_mismatch() {
    let path = write_temp_file("profile-schema-mismatch", "json", "{}\n");

    let error = run_profile_config_schema(ProfileConfigSchemaOptions {
        check: Some(path.clone()),
        ..Default::default()
    })
    .unwrap_err();
    remove_temp_file(path);

    assert!(error.to_string().contains("schema is out of date"));
}

#[test]
fn weixin_config_uses_env_values() {
    let env = BTreeMap::from([
        ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
        ("WEIXIN_ACCOUNT_ID", "wx-bot"),
        ("WEIXIN_TOKEN", "token"),
        ("WEIXIN_ALLOWED_USERS", "user-1,user-2"),
        ("WEIXIN_LOCALE", "zh"),
    ]);

    let config = weixin_config_from_values(&WeixinBridgeOptions::default(), |name| {
        env.get(name).map(|value| value.to_string())
    })
    .unwrap();

    assert_eq!(config.account_id, "wx-bot");
    assert!(config.access.allows_dm("user-1"));
    assert!(!config.access.allows_dm("user-3"));
    assert_eq!(config.locale, Locale::Zh);
}

#[test]
fn weixin_embedded_can_inherit_profile_locale() {
    let config =
        HostProfileConfig::load("examples/profile-configs/weixin-chatgpt-subscription.json")
            .unwrap();

    assert_eq!(profile_locale(&config, None), Some(Locale::Zh));
}

#[test]
fn weixin_config_rejects_missing_allowlist() {
    let env = BTreeMap::from([
        ("NOLOONG_INTERACTION_URL", "ws://127.0.0.1:8787/jsonrpc/ws"),
        ("WEIXIN_ACCOUNT_ID", "wx-bot"),
        ("WEIXIN_TOKEN", "token"),
    ]);

    let error = weixin_config_from_values(&WeixinBridgeOptions::default(), |name| {
        env.get(name).map(|value| value.to_string())
    })
    .unwrap_err();

    assert!(error.to_string().contains("allowlist"));
}
