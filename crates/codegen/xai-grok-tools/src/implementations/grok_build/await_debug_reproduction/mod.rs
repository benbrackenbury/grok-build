//! `AwaitDebugReproduction` — park until the user reproduces the bug.
//!
//! The shell intercepts this tool and presents Proceed / Abandon chrome via
//! ACP reverse-request. When the intercept is not wired, this tool returns a
//! prompt telling the model to wait for the user to confirm in chat.

use crate::types::output::AwaitDebugReproductionOutput;
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::resolve_debug_log_path;
use crate::types::tool::{ToolKind, ToolNamespace};

/// Input for awaiting user reproduction.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct AwaitDebugReproductionInput {
    /// Concrete steps the user should follow to reproduce the bug.
    #[schemars(description = "Markdown reproduction steps for the user.")]
    pub steps: String,
    /// Correlation id written into NDJSON events for this run (default `run1`).
    #[serde(default)]
    pub run_id: Option<String>,
}

/// `AwaitDebugReproduction` tool.
#[derive(Debug, Default)]
pub struct AwaitDebugReproductionTool;

impl crate::types::tool_metadata::ToolMetadata for AwaitDebugReproductionTool {
    fn kind(&self) -> ToolKind {
        ToolKind::DebugAwaitRepro
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        r#"After instrumenting the code, call this tool with concrete reproduction steps. The user will reproduce the bug while runtime logs are collected, then signal Proceed. Do not analyze logs until they proceed."#
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for AwaitDebugReproductionTool {
    type Args = AwaitDebugReproductionInput;
    type Output = AwaitDebugReproductionOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("await_debug_reproduction").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "await_debug_reproduction",
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

    #[tracing::instrument(name = "tool.await_debug_reproduction", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: AwaitDebugReproductionInput,
    ) -> Result<AwaitDebugReproductionOutput, xai_tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;
        let run_id = input
            .run_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("run1")
            .to_owned();
        let debug_log_path = {
            let res = resources.lock().await;
            resolve_debug_log_path(&res).1
        };

        // Shell HITL intercept replaces this result when wired. Fallback for
        // headless / incomplete wiring: instruct the model to wait in chat.
        Ok(AwaitDebugReproductionOutput::Waiting {
            message: format!(
                "Waiting for the user to reproduce the bug (runId={run_id}).\n\n\
                 Steps presented to the user:\n{}\n\n\
                 When they proceed, analyze NDJSON at {debug_log_path} for this runId. \
                 Do not invent log lines.",
                input.steps.trim()
            ),
            run_id,
            debug_log_path,
            steps: input.steps,
        })
    }
}

pub mod types;

pub use types::{AwaitDebugReproductionExtRequest, AwaitDebugReproductionExtResponse};
