use crate::{
    AgentCoreError, AgentEventKind, AgentMessage, AgentRuntime, AgentState, CancellationToken,
    ContentBlock, ContextCompactionConfig, ExtensionCapability, HttpAuthContext, HttpAuthProvider,
    HttpAuthRefreshContext, MessageRole, ModelStreamEvent, Result, RunReport, StdioExtension,
    StdioExtensionConfig, StdioHttpAuthProvider, ToolApprovalResolution, ToolPermissionDecision,
    ToolPermissionOutcome,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeStruct};
use std::{collections::BTreeSet, sync::Arc, time::Instant};

pub const CONFORMANCE_MODEL_PROVIDER_ID: &str = "conformance-model";
pub const CONFORMANCE_TOOL_NAME: &str = "conformance_echo";
pub const CONFORMANCE_CONTEXT_PROVIDER_ID: &str = "conformance-context";
pub const CONFORMANCE_PHASE_NODE_ID: &str = "conformance.phase";
pub const CONFORMANCE_PHASE_HOOK_ID: &str = "conformance-hook";
pub const CONFORMANCE_TOOL_CALL_HOOK_ID: &str = "conformance-tool-hook";
pub const CONFORMANCE_COMPACTION_SUMMARIZER_ID: &str = "conformance-compaction";
pub const CONFORMANCE_CONTEXT_COMPACTOR_ID: &str = "conformance-context-compactor";
pub const CONFORMANCE_HTTP_AUTH_PROVIDER_ID: &str = "conformance-auth";

const CASE_LIFECYCLE: &str = "lifecycle";
const CASE_RUNTIME_REGISTRATION: &str = "runtime_registration";
const CASE_STANDARD_CAPABILITIES: &str = "standard_capabilities";
const CASE_ADAPTER_PAYLOADS: &str = "adapter_payloads";
const CASE_TOOL_APPROVAL: &str = "tool_approval";
const CASE_COMPACTION_SUMMARIZER: &str = "compaction_summarizer";
const CASE_CONTEXT_COMPACTOR: &str = "context_compactor";
const CASE_HTTP_AUTH_PROVIDER: &str = "http_auth_provider";

const FULL_CASES: &[&str] = &[
    CASE_ADAPTER_PAYLOADS,
    CASE_TOOL_APPROVAL,
    CASE_COMPACTION_SUMMARIZER,
    CASE_CONTEXT_COMPACTOR,
    CASE_HTTP_AUTH_PROVIDER,
];

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionConformanceProfile {
    Generic,
    #[default]
    Hybrid,
    Strict,
}

impl ExtensionConformanceProfile {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Generic => "generic",
            Self::Hybrid => "hybrid",
            Self::Strict => "strict",
        }
    }

    pub fn from_name(value: &str) -> Option<Self> {
        match value {
            "generic" => Some(Self::Generic),
            "hybrid" => Some(Self::Hybrid),
            "strict" => Some(Self::Strict),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExtensionConformanceConfig {
    stdio: StdioExtensionConfig,
    profile: ExtensionConformanceProfile,
    fail_fast: bool,
}

impl ExtensionConformanceConfig {
    pub fn new(stdio: StdioExtensionConfig) -> Self {
        Self {
            stdio,
            profile: ExtensionConformanceProfile::default(),
            fail_fast: false,
        }
    }

    pub fn profile(mut self, profile: ExtensionConformanceProfile) -> Self {
        self.profile = profile;
        self
    }

    pub fn fail_fast(mut self, fail_fast: bool) -> Self {
        self.fail_fast = fail_fast;
        self
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionConformanceCaseStatus {
    Passed,
    Failed,
    Skipped,
}

impl ExtensionConformanceCaseStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionConformanceCaseReport {
    pub name: String,
    pub status: ExtensionConformanceCaseStatus,
    pub message: Option<String>,
    pub elapsed_ms: u128,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExtensionConformanceReport {
    pub profile: ExtensionConformanceProfile,
    pub cases: Vec<ExtensionConformanceCaseReport>,
}

impl ExtensionConformanceReport {
    pub fn new(profile: ExtensionConformanceProfile) -> Self {
        Self {
            profile,
            cases: Vec::new(),
        }
    }

    pub fn total(&self) -> usize {
        self.cases.len()
    }

    pub fn passed(&self) -> usize {
        self.count_status(ExtensionConformanceCaseStatus::Passed)
    }

    pub fn failed(&self) -> usize {
        self.count_status(ExtensionConformanceCaseStatus::Failed)
    }

    pub fn skipped(&self) -> usize {
        self.count_status(ExtensionConformanceCaseStatus::Skipped)
    }

    pub fn is_success(&self) -> bool {
        self.failed() == 0
    }

    fn push(&mut self, case: ExtensionConformanceCaseReport) {
        self.cases.push(case);
    }

    fn count_status(&self, status: ExtensionConformanceCaseStatus) -> usize {
        self.cases
            .iter()
            .filter(|case| case.status == status)
            .count()
    }
}

impl Serialize for ExtensionConformanceReport {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ExtensionConformanceReport", 6)?;
        state.serialize_field("profile", &self.profile)?;
        state.serialize_field("total", &self.total())?;
        state.serialize_field("passed", &self.passed())?;
        state.serialize_field("failed", &self.failed())?;
        state.serialize_field("skipped", &self.skipped())?;
        state.serialize_field("cases", &self.cases)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ExtensionConformanceReport {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct WireReport {
            profile: ExtensionConformanceProfile,
            total: usize,
            passed: usize,
            failed: usize,
            skipped: usize,
            cases: Vec<ExtensionConformanceCaseReport>,
        }

        let wire = WireReport::deserialize(deserializer)?;
        let report = Self {
            profile: wire.profile,
            cases: wire.cases,
        };
        if report.total() != wire.total
            || report.passed() != wire.passed
            || report.failed() != wire.failed
            || report.skipped() != wire.skipped
        {
            return Err(de::Error::custom(
                "extension conformance report counts do not match cases",
            ));
        }
        Ok(report)
    }
}

pub async fn run_extension_conformance(
    config: ExtensionConformanceConfig,
) -> Result<ExtensionConformanceReport> {
    let mut report = ExtensionConformanceReport::new(config.profile);

    let Some(capabilities) = run_case(
        &mut report,
        CASE_LIFECYCLE,
        inspect_lifecycle(&config.stdio),
    )
    .await
    else {
        push_skipped(&mut report, CASE_RUNTIME_REGISTRATION, "lifecycle failed");
        skip_full_cases(&mut report, "lifecycle failed");
        return Ok(report);
    };
    if should_stop(&report, config.fail_fast) {
        return Ok(report);
    }

    if run_case(
        &mut report,
        CASE_RUNTIME_REGISTRATION,
        register_extension(&config.stdio),
    )
    .await
    .is_none()
    {
        if should_stop(&report, config.fail_fast) {
            return Ok(report);
        }
        skip_full_cases(&mut report, "runtime registration failed");
        return Ok(report);
    }
    if should_stop(&report, config.fail_fast) {
        return Ok(report);
    }

    match full_conformance_decision(config.profile, &capabilities) {
        FullConformanceDecision::Run => {
            push_passed(&mut report, CASE_STANDARD_CAPABILITIES);
        }
        FullConformanceDecision::Skip(message) => {
            push_skipped(&mut report, CASE_STANDARD_CAPABILITIES, message.clone());
            skip_full_cases(&mut report, message);
            return Ok(report);
        }
        FullConformanceDecision::Fail(message) => {
            push_failed(&mut report, CASE_STANDARD_CAPABILITIES, message);
            if should_stop(&report, config.fail_fast) {
                return Ok(report);
            }
            skip_full_cases(&mut report, "standard capability check failed");
            return Ok(report);
        }
    }

    if config.fail_fast {
        run_case(
            &mut report,
            CASE_ADAPTER_PAYLOADS,
            adapter_payloads_case(&config.stdio),
        )
        .await;
        if should_stop(&report, config.fail_fast) {
            return Ok(report);
        }
        run_case(
            &mut report,
            CASE_COMPACTION_SUMMARIZER,
            compaction_summarizer_case(&config.stdio),
        )
        .await;
        if should_stop(&report, config.fail_fast) {
            return Ok(report);
        }
        run_case(
            &mut report,
            CASE_CONTEXT_COMPACTOR,
            context_compactor_case(&config.stdio),
        )
        .await;
        if should_stop(&report, config.fail_fast) {
            return Ok(report);
        }
        run_case(
            &mut report,
            CASE_HTTP_AUTH_PROVIDER,
            http_auth_provider_case(&config.stdio),
        )
        .await;
        if should_stop(&report, config.fail_fast) {
            return Ok(report);
        }
        run_case(
            &mut report,
            CASE_TOOL_APPROVAL,
            tool_approval_case(&config.stdio),
        )
        .await;
    } else {
        let (
            adapter_payloads,
            compaction_summarizer,
            context_compactor,
            http_auth_provider,
            tool_approval,
        ) = tokio::join!(
            evaluate_case(CASE_ADAPTER_PAYLOADS, adapter_payloads_case(&config.stdio)),
            evaluate_case(
                CASE_COMPACTION_SUMMARIZER,
                compaction_summarizer_case(&config.stdio)
            ),
            evaluate_case(
                CASE_CONTEXT_COMPACTOR,
                context_compactor_case(&config.stdio)
            ),
            evaluate_case(
                CASE_HTTP_AUTH_PROVIDER,
                http_auth_provider_case(&config.stdio)
            ),
            evaluate_case(CASE_TOOL_APPROVAL, tool_approval_case(&config.stdio))
        );
        report.push(adapter_payloads.0);
        report.push(compaction_summarizer.0);
        report.push(context_compactor.0);
        report.push(http_auth_provider.0);
        report.push(tool_approval.0);
    }

    Ok(report)
}

async fn run_case<T>(
    report: &mut ExtensionConformanceReport,
    name: &str,
    future: impl std::future::Future<Output = Result<T>>,
) -> Option<T> {
    let (case, value) = evaluate_case(name, future).await;
    report.push(case);
    value
}

async fn evaluate_case<T>(
    name: &str,
    future: impl std::future::Future<Output = Result<T>>,
) -> (ExtensionConformanceCaseReport, Option<T>) {
    let started = Instant::now();
    match future.await {
        Ok(value) => (
            ExtensionConformanceCaseReport {
                name: name.into(),
                status: ExtensionConformanceCaseStatus::Passed,
                message: None,
                elapsed_ms: started.elapsed().as_millis(),
            },
            Some(value),
        ),
        Err(error) => (
            ExtensionConformanceCaseReport {
                name: name.into(),
                status: ExtensionConformanceCaseStatus::Failed,
                message: Some(error.to_string()),
                elapsed_ms: started.elapsed().as_millis(),
            },
            None,
        ),
    }
}

fn push_passed(report: &mut ExtensionConformanceReport, name: &str) {
    report.push(ExtensionConformanceCaseReport {
        name: name.into(),
        status: ExtensionConformanceCaseStatus::Passed,
        message: None,
        elapsed_ms: 0,
    });
}

fn push_failed(report: &mut ExtensionConformanceReport, name: &str, message: impl Into<String>) {
    report.push(ExtensionConformanceCaseReport {
        name: name.into(),
        status: ExtensionConformanceCaseStatus::Failed,
        message: Some(message.into()),
        elapsed_ms: 0,
    });
}

fn push_skipped(report: &mut ExtensionConformanceReport, name: &str, message: impl Into<String>) {
    report.push(ExtensionConformanceCaseReport {
        name: name.into(),
        status: ExtensionConformanceCaseStatus::Skipped,
        message: Some(message.into()),
        elapsed_ms: 0,
    });
}

fn skip_full_cases(report: &mut ExtensionConformanceReport, message: impl Into<String>) {
    let message = message.into();
    for case in FULL_CASES {
        push_skipped(report, case, message.clone());
    }
}

fn should_stop(report: &ExtensionConformanceReport, fail_fast: bool) -> bool {
    fail_fast && report.failed() > 0
}

async fn inspect_lifecycle(stdio: &StdioExtensionConfig) -> Result<Vec<ExtensionCapability>> {
    let extension = StdioExtension::connect(stdio.clone()).await?;
    ensure(
        !extension.manifest().name.trim().is_empty(),
        "manifest name must not be empty",
    )?;
    ensure(
        !extension.manifest().version.trim().is_empty(),
        "manifest version must not be empty",
    )?;
    let capabilities = extension.capabilities().await?;
    extension.shutdown().await?;
    Ok(capabilities)
}

async fn register_extension(stdio: &StdioExtensionConfig) -> Result<()> {
    AgentRuntime::builder()
        .with_stdio_extension(stdio.clone())
        .await?;
    Ok(())
}

async fn adapter_payloads_case(stdio: &StdioExtensionConfig) -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_stdio_extension(stdio.clone())
        .await?
        .max_turns(2)
        .build()?;
    let report = runtime.run("adapter").await?;

    ensure_assistant_text_contains(&report, "adapter complete")?;
    ensure(
        report.state.context.get("conformance_context") == Some(&serde_json::json!(true)),
        "context provider did not apply conformance_context",
    )?;
    ensure(
        report.state.context.get("conformance_phase") == Some(&serde_json::json!(true)),
        "phase node did not apply conformance_phase",
    )?;
    ensure(
        report.events.iter().any(|event| {
            matches!(
                &event.kind,
                AgentEventKind::ToolExecutionUpdate { update, .. }
                    if update.content.iter().any(|block| {
                        matches!(block, ContentBlock::Text { text } if text == "tool update")
                    })
            )
        }),
        "tool provider did not emit expected update",
    )?;
    ensure(
        report.events.iter().any(|event| {
            matches!(
                &event.kind,
                AgentEventKind::ToolPermissionDecided { decision, .. }
                    if decision.outcome == ToolPermissionOutcome::Allow
            )
        }),
        "tool call hook did not produce an allow permission decision",
    )?;
    ensure(
        report.events.iter().any(|event| {
            matches!(
                &event.kind,
                AgentEventKind::ModelStreamEvent {
                    event: ModelStreamEvent::TextDelta { .. },
                    ..
                }
            )
        }),
        "model provider did not emit text deltas",
    )
}

async fn tool_approval_case(stdio: &StdioExtensionConfig) -> Result<()> {
    let runtime = AgentRuntime::builder()
        .with_stdio_extension(stdio.clone())
        .await?
        .max_turns(2)
        .build()?;
    let paused = runtime.run("approval").await?;
    ensure(
        matches!(paused.state.status, crate::RunStatus::Paused),
        "tool approval run did not pause",
    )?;
    ensure(
        paused.state.pending_tool_approvals.len() == 1,
        "tool approval run did not expose exactly one pending approval",
    )?;
    let approval_id = paused
        .state
        .pending_tool_approvals
        .keys()
        .next()
        .cloned()
        .ok_or_else(|| AgentCoreError::JsonRpc("tool approval id missing".into()))?;
    ensure(
        paused.events.iter().any(|event| {
            matches!(&event.kind, AgentEventKind::ToolApprovalRequested { approval }
                if approval.hook_id.as_deref() == Some(CONFORMANCE_TOOL_CALL_HOOK_ID))
        }),
        "tool approval request event missing",
    )?;

    let report = runtime
        .resume_tool_approvals(
            &paused.run_id,
            vec![ToolApprovalResolution {
                approval_id,
                decision: ToolPermissionDecision {
                    outcome: ToolPermissionOutcome::Allow,
                    reason: Some("approved by conformance runner".into()),
                    approver: Some("conformance-runner".into()),
                    metadata: serde_json::json!({}),
                },
            }],
            None,
            CancellationToken::new(),
        )
        .await?;
    ensure_assistant_text_contains(&report, "adapter complete")?;
    ensure(
        report.events.iter().any(|event| {
            matches!(
                &event.kind,
                AgentEventKind::ToolPermissionDecided {
                    hook_id,
                    decision,
                    ..
                } if hook_id.as_deref() == Some(CONFORMANCE_TOOL_CALL_HOOK_ID)
                    && decision.outcome == ToolPermissionOutcome::Allow
                    && decision.approver.as_deref() == Some("conformance-runner")
            )
        }),
        "tool approval decision was not replayed into permission audit",
    )
}

async fn compaction_summarizer_case(stdio: &StdioExtensionConfig) -> Result<()> {
    let builder = AgentRuntime::builder()
        .with_stdio_extension(stdio.clone())
        .await?;
    let runtime = builder
        .with_context_compaction_summarizer_id(
            ContextCompactionConfig::new(64)
                .summary_budget_tokens(8)
                .keep_recent_tokens(10),
            CONFORMANCE_COMPACTION_SUMMARIZER_ID,
        )
        .max_turns(1)
        .build()?;

    let report = runtime
        .continue_from_state(
            conformance_compaction_trigger_state(),
            None,
            CancellationToken::new(),
        )
        .await?;

    ensure(
        report.state.messages.iter().any(|message| {
            matches!(message.role, MessageRole::System)
                && message.content.iter().any(|block| {
                    matches!(
                        block,
                        ContentBlock::Text { text }
                            if text.contains("conformance compaction summary")
                    )
                })
        }),
        "compaction summarizer did not produce expected summary",
    )
}

async fn context_compactor_case(stdio: &StdioExtensionConfig) -> Result<()> {
    let builder = AgentRuntime::builder()
        .with_stdio_extension(stdio.clone())
        .await?;
    let runtime = builder
        .with_context_compactor_id(
            ContextCompactionConfig::new(64)
                .summary_budget_tokens(8)
                .keep_recent_tokens(10),
            CONFORMANCE_CONTEXT_COMPACTOR_ID,
        )
        .max_turns(1)
        .build()?;

    let report = runtime
        .continue_from_state(
            conformance_compaction_trigger_state(),
            None,
            CancellationToken::new(),
        )
        .await?;

    ensure(
        report.state.messages.iter().any(|message| {
            message.content.iter().any(|block| {
                matches!(
                    block,
                    ContentBlock::Text { text }
                        if text.contains("conformance replacement summary")
                )
            })
        }),
        "context compactor did not produce expected replacement history",
    )?;
    ensure(
        report.events.iter().any(|event| {
            matches!(
                &event.kind,
                AgentEventKind::EffectCommitted {
                    effect: crate::AgentEffect::ReplaceMessages { .. }
                }
            )
        }),
        "context compactor did not commit a replacement effect",
    )
}

async fn http_auth_provider_case(stdio: &StdioExtensionConfig) -> Result<()> {
    let extension = Arc::new(StdioExtension::connect(stdio.clone()).await?);
    let auth = StdioHttpAuthProvider::new(extension, CONFORMANCE_HTTP_AUTH_PROVIDER_ID.into());
    let context = HttpAuthContext::new("conformance-provider", "POST", "https://example.test", 0);
    let headers = auth
        .headers(context.clone(), CancellationToken::new())
        .await?;
    ensure(
        headers.headers.iter().any(|header| {
            header.name.eq_ignore_ascii_case("authorization")
                && header.value == "Bearer conformance-auth"
        }),
        "http auth provider did not return expected authorization header",
    )?;

    let refresh = auth
        .refresh(
            HttpAuthRefreshContext::unauthorized(context, 401),
            CancellationToken::new(),
        )
        .await?;
    ensure(refresh.retry, "http auth provider refresh denied retry")?;
    ensure(
        refresh.headers.as_ref().is_some_and(|headers| {
            headers.iter().any(|header| {
                header.name.eq_ignore_ascii_case("authorization")
                    && header.value == "Bearer conformance-refresh"
            })
        }),
        "http auth provider refresh did not return expected authorization header",
    )
}

enum FullConformanceDecision {
    Run,
    Skip(String),
    Fail(String),
}

fn full_conformance_decision(
    profile: ExtensionConformanceProfile,
    capabilities: &[ExtensionCapability],
) -> FullConformanceDecision {
    let presence = StandardCapabilityPresence::from_capabilities(capabilities);
    if matches!(profile, ExtensionConformanceProfile::Generic) {
        return FullConformanceDecision::Skip("profile is generic".into());
    }
    if presence.is_complete() {
        return FullConformanceDecision::Run;
    }
    if matches!(profile, ExtensionConformanceProfile::Strict) {
        return FullConformanceDecision::Fail(format!(
            "missing standard conformance capabilities: {}",
            presence.missing().join(", ")
        ));
    }
    if presence.is_absent() || presence.is_model_only() {
        return FullConformanceDecision::Skip(
            "extension does not advertise full conformance capabilities".into(),
        );
    }
    FullConformanceDecision::Fail(format!(
        "partial standard conformance capability set; missing: {}",
        presence.missing().join(", ")
    ))
}

const STANDARD_CAPABILITIES: &[StandardCapability] = &[
    StandardCapability::ModelProvider,
    StandardCapability::Tool,
    StandardCapability::ContextProvider,
    StandardCapability::PhaseNode,
    StandardCapability::PhaseHook,
    StandardCapability::ToolCallHook,
    StandardCapability::CompactionSummarizer,
    StandardCapability::ContextCompactor,
    StandardCapability::HttpAuthProvider,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum StandardCapability {
    ModelProvider,
    Tool,
    ContextProvider,
    PhaseNode,
    PhaseHook,
    ToolCallHook,
    CompactionSummarizer,
    ContextCompactor,
    HttpAuthProvider,
}

impl StandardCapability {
    fn id(self) -> &'static str {
        match self {
            Self::ModelProvider => CONFORMANCE_MODEL_PROVIDER_ID,
            Self::Tool => CONFORMANCE_TOOL_NAME,
            Self::ContextProvider => CONFORMANCE_CONTEXT_PROVIDER_ID,
            Self::PhaseNode => CONFORMANCE_PHASE_NODE_ID,
            Self::PhaseHook => CONFORMANCE_PHASE_HOOK_ID,
            Self::ToolCallHook => CONFORMANCE_TOOL_CALL_HOOK_ID,
            Self::CompactionSummarizer => CONFORMANCE_COMPACTION_SUMMARIZER_ID,
            Self::ContextCompactor => CONFORMANCE_CONTEXT_COMPACTOR_ID,
            Self::HttpAuthProvider => CONFORMANCE_HTTP_AUTH_PROVIDER_ID,
        }
    }

    fn matches(self, capability: &ExtensionCapability) -> bool {
        match (self, capability) {
            (Self::ModelProvider, ExtensionCapability::ModelProvider { id }) => {
                id == CONFORMANCE_MODEL_PROVIDER_ID
            }
            (Self::Tool, ExtensionCapability::Tool { spec }) => spec.name == CONFORMANCE_TOOL_NAME,
            (Self::ContextProvider, ExtensionCapability::ContextProvider { id }) => {
                id == CONFORMANCE_CONTEXT_PROVIDER_ID
            }
            (Self::PhaseNode, ExtensionCapability::PhaseNode { id }) => {
                id == CONFORMANCE_PHASE_NODE_ID
            }
            (Self::PhaseHook, ExtensionCapability::PhaseHook { id }) => {
                id == CONFORMANCE_PHASE_HOOK_ID
            }
            (Self::ToolCallHook, ExtensionCapability::ToolCallHook { id }) => {
                id == CONFORMANCE_TOOL_CALL_HOOK_ID
            }
            (Self::CompactionSummarizer, ExtensionCapability::CompactionSummarizer { id }) => {
                id == CONFORMANCE_COMPACTION_SUMMARIZER_ID
            }
            (Self::ContextCompactor, ExtensionCapability::ContextCompactor { id }) => {
                id == CONFORMANCE_CONTEXT_COMPACTOR_ID
            }
            (Self::HttpAuthProvider, ExtensionCapability::HttpAuthProvider { id }) => {
                id == CONFORMANCE_HTTP_AUTH_PROVIDER_ID
            }
            _ => false,
        }
    }
}

struct StandardCapabilityPresence {
    present: BTreeSet<StandardCapability>,
}

impl StandardCapabilityPresence {
    fn from_capabilities(capabilities: &[ExtensionCapability]) -> Self {
        let present = STANDARD_CAPABILITIES
            .iter()
            .copied()
            .filter(|standard| {
                capabilities
                    .iter()
                    .any(|capability| standard.matches(capability))
            })
            .collect();
        Self { present }
    }

    fn is_complete(&self) -> bool {
        self.present.len() == STANDARD_CAPABILITIES.len()
    }

    fn is_absent(&self) -> bool {
        self.present.is_empty()
    }

    fn is_model_only(&self) -> bool {
        self.present.len() == 1 && self.present.contains(&StandardCapability::ModelProvider)
    }

    fn missing(&self) -> Vec<&'static str> {
        STANDARD_CAPABILITIES
            .iter()
            .copied()
            .filter_map(|capability| {
                (!self.present.contains(&capability)).then_some(capability.id())
            })
            .collect()
    }
}

fn conformance_compaction_trigger_state() -> AgentState {
    AgentState {
        messages: vec![
            AgentMessage::user("u1", "old ".repeat(80)),
            AgentMessage::assistant(
                "a1",
                vec![ContentBlock::Text {
                    text: "old answer ".repeat(80),
                }],
            ),
            AgentMessage::user("u2", "recent"),
        ],
        ..AgentState::default()
    }
}

fn ensure_assistant_text_contains(report: &RunReport, expected: &str) -> Result<()> {
    if assistant_text_blocks(report).any(|text| text.contains(expected)) {
        return Ok(());
    }

    let preview = assistant_visible_text_preview(report, 4096);
    ensure(
        false,
        format!("assistant visible text did not contain `{expected}`: {preview}"),
    )
}

fn assistant_text_blocks(report: &RunReport) -> impl Iterator<Item = &str> + '_ {
    report
        .state
        .messages
        .iter()
        .filter(|message| matches!(message.role, MessageRole::Assistant))
        .flat_map(|message| &message.content)
        .filter_map(|block| match block {
            ContentBlock::Text { text } => Some(text.as_str()),
            _ => None,
        })
}

fn assistant_visible_text_preview(report: &RunReport, max_bytes: usize) -> String {
    let mut preview = String::new();
    for text in assistant_text_blocks(report) {
        for ch in text.chars() {
            if preview.len() + ch.len_utf8() > max_bytes {
                preview.push_str("...");
                return preview;
            }
            preview.push(ch);
        }
    }
    preview
}

fn ensure(condition: bool, message: impl Into<String>) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(AgentCoreError::JsonRpc(format!(
            "extension conformance failed: {}",
            message.into()
        )))
    }
}
