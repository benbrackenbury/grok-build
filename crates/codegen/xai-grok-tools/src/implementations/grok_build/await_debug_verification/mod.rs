//! `AwaitDebugVerification` — park until the user marks fixed or still-broken.

use crate::types::output::AwaitDebugVerificationOutput;
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::resolve_debug_log_path;
use crate::types::tool::{ToolKind, ToolNamespace};

/// Input for awaiting user verification.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AwaitDebugVerificationInput {
    /// Short summary of the fix for the user to verify.
    #[schemars(description = "What was changed and how the user should re-check.")]
    pub summary: String,
    /// Correlation id for post-fix logs (default `post-fix`).
    #[serde(default)]
    pub run_id: Option<String>,
}

/// `AwaitDebugVerification` tool.
#[derive(Debug, Default)]
pub struct AwaitDebugVerificationTool;

impl crate::types::tool_metadata::ToolMetadata for AwaitDebugVerificationTool {
    fn kind(&self) -> ToolKind {
        ToolKind::DebugAwaitVerify
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        r#"After applying a fix (keeping instrumentation), call this so the user can re-reproduce and Mark Fixed or Still broken."#
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for AwaitDebugVerificationTool {
    type Args = AwaitDebugVerificationInput;
    type Output = AwaitDebugVerificationOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("await_debug_verification").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "await_debug_verification",
            crate::types::tool_metadata::ToolMetadata::description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: true,
            tool_scope: Some(xai_tool_protocol::ToolScope::Read),
            ..Default::default()
        }
    }

    #[tracing::instrument(name = "tool.await_debug_verification", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: AwaitDebugVerificationInput,
    ) -> Result<AwaitDebugVerificationOutput, xai_tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;
        let run_id = input
            .run_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("post-fix")
            .to_owned();
        let debug_log_path = {
            let res = resources.lock().await;
            resolve_debug_log_path(&res).1
        };

        Ok(AwaitDebugVerificationOutput::Waiting {
            message: format!(
                "Waiting for the user to verify the fix (runId={run_id}).\n\n\
                 Summary shown to the user:\n{}\n\n\
                 On Mark Fixed: remove all agent-log regions, then exit debug mode.\n\
                 On Still broken: refine hypotheses/instrumentation and loop.\n\
                 Log path: {debug_log_path}",
                input.summary.trim()
            ),
            run_id,
            debug_log_path,
            summary: input.summary,
        })
    }
}

pub mod types;

pub use types::{AwaitDebugVerificationExtRequest, AwaitDebugVerificationExtResponse};
