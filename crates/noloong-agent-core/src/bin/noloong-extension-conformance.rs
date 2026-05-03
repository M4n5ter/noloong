use noloong_agent_core::{
    ExtensionConformanceConfig, ExtensionConformanceProfile, ExtensionConformanceReport, Result,
    StdioExtensionConfig, run_extension_conformance,
};
use std::{env, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(1)
        }
    }
}

async fn run() -> Result<ExitCode> {
    let cli = match CliArgs::parse(env::args().skip(1).collect()) {
        Ok(cli) => cli,
        Err(message) => {
            eprintln!("{message}");
            eprintln!("{}", usage());
            return Ok(ExitCode::from(2));
        }
    };
    let stdio = StdioExtensionConfig::new(cli.command).args(cli.args);
    let report = run_extension_conformance(
        ExtensionConformanceConfig::new(stdio)
            .profile(cli.profile)
            .fail_fast(cli.fail_fast),
    )
    .await?;
    if cli.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_text_report(&report);
    }
    Ok(if report.is_success() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

#[derive(Debug, PartialEq, Eq)]
struct CliArgs {
    profile: ExtensionConformanceProfile,
    json: bool,
    fail_fast: bool,
    command: String,
    args: Vec<String>,
}

impl CliArgs {
    fn parse(args: Vec<String>) -> std::result::Result<Self, String> {
        let Some(separator) = args.iter().position(|arg| arg == "--") else {
            return Err("missing `-- <command> [args...]`".into());
        };
        let (runner_args, command_args) = args.split_at(separator);
        let command_args = &command_args[1..];
        let Some(command) = command_args.first() else {
            return Err("missing extension command after `--`".into());
        };

        let mut profile = ExtensionConformanceProfile::default();
        let mut json = false;
        let mut fail_fast = false;
        let mut index = 0;
        while index < runner_args.len() {
            let arg = &runner_args[index];
            match arg.as_str() {
                "--json" => {
                    json = true;
                    index += 1;
                }
                "--fail-fast" => {
                    fail_fast = true;
                    index += 1;
                }
                "--profile" => {
                    let Some(value) = runner_args.get(index + 1) else {
                        return Err("missing value for `--profile`".into());
                    };
                    profile = parse_profile(value)?;
                    index += 2;
                }
                _ if arg.starts_with("--profile=") => {
                    profile = parse_profile(&arg["--profile=".len()..])?;
                    index += 1;
                }
                _ => return Err(format!("unknown argument: {arg}")),
            }
        }

        Ok(Self {
            profile,
            json,
            fail_fast,
            command: command.clone(),
            args: command_args[1..].to_vec(),
        })
    }
}

fn parse_profile(value: &str) -> std::result::Result<ExtensionConformanceProfile, String> {
    ExtensionConformanceProfile::from_name(value).ok_or_else(|| {
        format!("invalid profile `{value}`; expected one of: generic, hybrid, strict")
    })
}

fn print_text_report(report: &ExtensionConformanceReport) {
    println!(
        "extension conformance profile={} total={} passed={} failed={} skipped={}",
        report.profile.as_str(),
        report.total(),
        report.passed(),
        report.failed(),
        report.skipped()
    );
    for case in &report.cases {
        match &case.message {
            Some(message) => println!(
                "[{}] {} ({} ms): {}",
                case.status.as_str(),
                case.name,
                case.elapsed_ms,
                message
            ),
            None => println!(
                "[{}] {} ({} ms)",
                case.status.as_str(),
                case.name,
                case.elapsed_ms
            ),
        }
    }
}

fn usage() -> &'static str {
    "usage: noloong-extension-conformance [--profile generic|hybrid|strict] [--json] [--fail-fast] -- <command> [args...]"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_profile_json_fail_fast_and_command() {
        let args = CliArgs::parse(vec![
            "--profile".into(),
            "strict".into(),
            "--json".into(),
            "--fail-fast".into(),
            "--".into(),
            "node".into(),
            "fixture.mjs".into(),
        ])
        .expect("parse args");

        assert_eq!(
            args,
            CliArgs {
                profile: ExtensionConformanceProfile::Strict,
                json: true,
                fail_fast: true,
                command: "node".into(),
                args: vec!["fixture.mjs".into()],
            }
        );
    }

    #[test]
    fn rejects_missing_command_separator() {
        let error = CliArgs::parse(vec!["--profile".into(), "hybrid".into()])
            .expect_err("missing separator should fail");

        assert!(error.contains("missing"));
    }
}
