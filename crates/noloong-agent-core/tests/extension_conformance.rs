use noloong_agent_core::{
    ExtensionConformanceCaseStatus, ExtensionConformanceConfig, ExtensionConformanceProfile,
    ExtensionConformanceReport, Result, run_extension_conformance,
};

pub mod support;

use support::jsonrpc_conformance_config as config;

const MODE_ALL_CAPABILITIES: &str = "all-capabilities";
const MODE_ADAPTER_PAYLOADS: &str = "adapter-payloads";
const MODE_PARTIAL_CONFORMANCE: &str = "partial-conformance";
const MODE_TOOL_HOOK_PAYLOADS: &str = "tool-hook-payloads";

#[tokio::test]
async fn strict_profile_passes_full_fixture() -> Result<()> {
    let report = run_extension_conformance(
        ExtensionConformanceConfig::new(config(&[
            MODE_ALL_CAPABILITIES,
            MODE_ADAPTER_PAYLOADS,
            MODE_TOOL_HOOK_PAYLOADS,
        ]))
        .profile(ExtensionConformanceProfile::Strict),
    )
    .await?;

    assert!(report.is_success());
    assert_eq!(report.failed(), 0);
    assert_eq!(report.skipped(), 0);
    assert_case_status(
        &report,
        "standard_capabilities",
        ExtensionConformanceCaseStatus::Passed,
    );
    assert_case_status(
        &report,
        "adapter_payloads",
        ExtensionConformanceCaseStatus::Passed,
    );
    assert_case_status(
        &report,
        "compaction_summarizer",
        ExtensionConformanceCaseStatus::Passed,
    );
    Ok(())
}

#[tokio::test]
async fn hybrid_profile_skips_full_cases_for_model_only_fixture() -> Result<()> {
    let report =
        run_extension_conformance(ExtensionConformanceConfig::new(config(&[])).fail_fast(false))
            .await?;

    assert!(report.is_success());
    assert_eq!(report.failed(), 0);
    assert!(report.skipped() >= 2);
    assert_case_status(&report, "lifecycle", ExtensionConformanceCaseStatus::Passed);
    assert_case_status(
        &report,
        "runtime_registration",
        ExtensionConformanceCaseStatus::Passed,
    );
    assert_case_status(
        &report,
        "standard_capabilities",
        ExtensionConformanceCaseStatus::Skipped,
    );
    assert_case_status(
        &report,
        "adapter_payloads",
        ExtensionConformanceCaseStatus::Skipped,
    );
    Ok(())
}

#[tokio::test]
async fn hybrid_profile_fails_partial_standard_capability_set() -> Result<()> {
    let report = run_extension_conformance(ExtensionConformanceConfig::new(config(&[
        MODE_PARTIAL_CONFORMANCE,
    ])))
    .await?;

    assert!(!report.is_success());
    assert_eq!(report.failed(), 1);
    let case = report
        .cases
        .iter()
        .find(|case| case.name == "standard_capabilities")
        .expect("standard capability case");
    assert_eq!(case.status, ExtensionConformanceCaseStatus::Failed);
    assert!(
        case.message
            .as_deref()
            .is_some_and(|message| message.contains("partial standard conformance"))
    );
    Ok(())
}

#[tokio::test]
async fn fail_fast_stops_after_first_failure() -> Result<()> {
    let report = run_extension_conformance(
        ExtensionConformanceConfig::new(config(&[MODE_PARTIAL_CONFORMANCE])).fail_fast(true),
    )
    .await?;

    assert!(!report.is_success());
    assert_eq!(report.failed(), 1);
    assert_case_status(
        &report,
        "standard_capabilities",
        ExtensionConformanceCaseStatus::Failed,
    );
    assert!(
        report
            .cases
            .iter()
            .all(|case| case.name != "adapter_payloads")
    );
    Ok(())
}

#[tokio::test]
async fn report_serde_round_trip_preserves_counts_and_case_status() -> Result<()> {
    let report = run_extension_conformance(ExtensionConformanceConfig::new(config(&[]))).await?;

    let encoded = serde_json::to_string(&report)?;
    let decoded: ExtensionConformanceReport = serde_json::from_str(&encoded)?;

    assert_eq!(decoded.profile, report.profile);
    assert_eq!(decoded.total(), report.total());
    assert_eq!(decoded.passed(), report.passed());
    assert_eq!(decoded.failed(), report.failed());
    assert_eq!(decoded.skipped(), report.skipped());
    assert_eq!(decoded.cases, report.cases);
    Ok(())
}

fn assert_case_status(
    report: &noloong_agent_core::ExtensionConformanceReport,
    name: &str,
    status: ExtensionConformanceCaseStatus,
) {
    assert_eq!(
        report
            .cases
            .iter()
            .find(|case| case.name == name)
            .map(|case| &case.status),
        Some(&status),
        "case `{name}` did not have expected status `{}`; report: {report:?}",
        status.as_str()
    );
}
