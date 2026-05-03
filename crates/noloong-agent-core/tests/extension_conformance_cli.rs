use noloong_agent_core::ExtensionConformanceReport;
use std::process::Command;

pub mod support;

use support::fixture_path;

#[test]
fn cli_strict_fixture_smoke_succeeds() {
    let output = command()
        .args([
            "--profile",
            "strict",
            "--",
            "node",
            fixture().as_str(),
            "--mode=all-capabilities,adapter-payloads,tool-hook-payloads",
        ])
        .output()
        .expect("run conformance cli");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("profile=strict"));
    assert!(stdout.contains("failed=0"));
}

#[test]
fn cli_hybrid_model_only_smoke_succeeds_with_skipped_cases() {
    let output = command()
        .args(["--profile", "hybrid", "--", "node", fixture().as_str()])
        .output()
        .expect("run conformance cli");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("profile=hybrid"));
    assert!(stdout.contains("failed=0"));
    assert!(stdout.contains("[skipped] adapter_payloads"));
}

#[test]
fn cli_json_output_deserializes_to_report() {
    let output = command()
        .args([
            "--profile",
            "hybrid",
            "--json",
            "--",
            "node",
            fixture().as_str(),
        ])
        .output()
        .expect("run conformance cli");

    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let report: ExtensionConformanceReport =
        serde_json::from_slice(&output.stdout).expect("parse JSON report");

    assert!(report.is_success());
    assert_eq!(report.failed(), 0);
    assert!(report.total() >= 5);
}

#[test]
fn cli_invalid_profile_fails() {
    let output = command()
        .args(["--profile", "invalid", "--", "node", fixture().as_str()])
        .output()
        .expect("run conformance cli");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("invalid profile"));
}

#[test]
fn cli_missing_command_fails() {
    let output = command()
        .args(["--profile", "hybrid"])
        .output()
        .expect("run conformance cli");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("missing"));
}

fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_noloong-extension-conformance"))
}

fn fixture() -> String {
    fixture_path("jsonrpc-conformance-extension.mjs")
        .to_string_lossy()
        .into_owned()
}
