use noloong_agent_core::{
    ExtensionConformanceConfig, ExtensionConformanceProfile, ExtensionConformanceReport, Result,
    StdioExtensionConfig, run_extension_conformance,
};
pub mod support;
use std::{
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
use support::{init_test_logger, workspace_root};

#[tokio::test]
async fn python_conformance_example_passes_strict_profile() -> Result<()> {
    let extension = workspace_root()
        .join("examples")
        .join("extensions")
        .join("python-conformance")
        .join("full_conformance_extension.py");

    let report = run_extension_conformance(
        ExtensionConformanceConfig::new(
            StdioExtensionConfig::new("python3")
                .arg(extension.to_string_lossy())
                .request_timeout(Duration::from_secs(5))
                .stream_timeout(Duration::from_secs(5)),
        )
        .profile(ExtensionConformanceProfile::Strict),
    )
    .await?;

    assert_strict_report("Python", &report);
    Ok(())
}

#[tokio::test]
async fn typescript_conformance_example_passes_strict_profile_when_dependencies_are_available()
-> Result<()> {
    let example_dir = workspace_root()
        .join("examples")
        .join("extensions")
        .join("typescript-conformance");
    let extension = example_dir
        .join("src")
        .join("full-conformance-extension.ts");

    let Some(tsx_command) = tsx_command(&example_dir) else {
        init_test_logger();
        log::info!(
            "skipping TypeScript conformance example; run `npm install` in {}",
            example_dir.display()
        );
        return Ok(());
    };

    let report = run_extension_conformance(
        ExtensionConformanceConfig::new(
            StdioExtensionConfig::new(tsx_command)
                .arg(extension.to_string_lossy())
                .request_timeout(Duration::from_secs(5))
                .stream_timeout(Duration::from_secs(5)),
        )
        .profile(ExtensionConformanceProfile::Strict),
    )
    .await?;

    assert_strict_report("TypeScript", &report);
    Ok(())
}

fn assert_strict_report(language: &str, report: &ExtensionConformanceReport) {
    assert!(
        report.is_success(),
        "{language} strict conformance failed: {report:?}"
    );
    assert_eq!(report.failed(), 0, "{language} report had failed cases");
    assert_eq!(report.skipped(), 0, "{language} report had skipped cases");
}

fn tsx_command(example_dir: &Path) -> Option<String> {
    let local = example_dir
        .join("node_modules")
        .join(".bin")
        .join(if cfg!(windows) { "tsx.cmd" } else { "tsx" });
    if local.exists() {
        return Some(local.to_string_lossy().into_owned());
    }
    command_exists("tsx").then(|| "tsx".to_string())
}

fn command_exists(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}
