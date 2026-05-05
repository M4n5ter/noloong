use crate::{Catalog, ManifestPatch, ManifestProposalStore, MessageKey, ProductToolName};
use noloong_agent_core::{
    BoxFuture, CancellationToken, ToolOutput, ToolProvider, ToolRequest, ToolSpec,
};
use serde_json::json;

use super::{json_tool_output, sequential_tool_spec};

#[derive(Clone)]
pub struct ManifestPatchProposalTool {
    store: ManifestProposalStore,
    catalog: Catalog,
}

impl ManifestPatchProposalTool {
    pub fn new(store: ManifestProposalStore, catalog: Catalog) -> Self {
        Self { store, catalog }
    }
}

impl ToolProvider for ManifestPatchProposalTool {
    fn spec(&self) -> ToolSpec {
        sequential_tool_spec(
            ProductToolName::ManifestProposePatch.as_str(),
            self.catalog.message(MessageKey::ManifestPatchDescription),
            json!({
                "type": "object",
                "required": ["patch"],
                "properties": {
                    "patch": {"type": "object"}
                }
            }),
            "agent.manifest.patch",
            self.catalog
                .message(MessageKey::ManifestPatchPermissionDescription),
        )
    }

    fn execute_tool<'a>(
        &'a self,
        request: ToolRequest,
        cancellation: CancellationToken,
    ) -> BoxFuture<'a, ToolOutput> {
        Box::pin(async move {
            cancellation.throw_if_cancelled()?;
            let patch_value = request.arguments.get("patch").cloned().ok_or_else(|| {
                noloong_agent_core::AgentCoreError::InvalidEffect(
                    self.catalog.missing_manifest_patch_argument().into(),
                )
            })?;
            let patch = serde_json::from_value::<ManifestPatch>(patch_value).map_err(|error| {
                noloong_agent_core::AgentCoreError::InvalidEffect(
                    self.catalog.render_tool_input_error(error),
                )
            })?;
            let summary = self.catalog.render_manifest_patch_summary(&patch);
            let proposal = self
                .store
                .record_pending_proposal_with_summary(patch, Some(summary))
                .map_err(|error| {
                    noloong_agent_core::AgentCoreError::Provider(
                        self.catalog.render_manifest_error(&error),
                    )
                })?;
            let value = json!(proposal);
            Ok(json_tool_output(value))
        })
    }
}
