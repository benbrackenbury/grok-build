//! `ExitDebugMode` tool — leave debug mode after cleanup.

use crate::notification::types::DebugModeExited;
use crate::types::output::ExitDebugModeOutput;
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::{NotificationHandle, resolve_debug_log_path};
use crate::types::tool::{ToolKind, ToolNamespace};

/// Input for the `ExitDebugMode` tool.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ExitDebugModeInput {
    /// Optional note about residual instrumentation or outcome.
    #[serde(default)]
    pub note: Option<String>,
}

/// `ExitDebugMode` tool.
#[derive(Debug, Default)]
pub struct ExitDebugModeTool;

impl crate::types::tool_metadata::ToolMetadata for ExitDebugModeTool {
    fn kind(&self) -> ToolKind {
        ToolKind::ExitDebug
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn emitted_notifications(&self) -> &'static [&'static str] {
        &["DebugModeExited"]
    }

    fn description_template(&self) -> &str {
        r#"Exit debug mode after removing instrumentation and finishing the debug loop. Call only after cleanup (or when abandoning with residual-log warning)."#
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        use crate::implementations::grok_build::enter_debug_mode::EnterDebugModeTool;
        Expr::Value(ToolRequirement::Tool {
            namespace: crate::types::tool_metadata::ToolMetadata::tool_namespace(
                &EnterDebugModeTool,
            )
            .to_string(),
            id: xai_tool_runtime::Tool::id(&EnterDebugModeTool).to_string(),
            if_params: None,
        })
    }
}

impl xai_tool_runtime::Tool for ExitDebugModeTool {
    type Args = ExitDebugModeInput;
    type Output = ExitDebugModeOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("exit_debug_mode").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "exit_debug_mode",
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

    #[tracing::instrument(name = "tool.exit_debug_mode", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: ExitDebugModeInput,
    ) -> Result<ExitDebugModeOutput, xai_tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;

        let debug_log_path = {
            let res = resources.lock().await;
            let (_, display) = resolve_debug_log_path(&res);
            if let Some(handle) = res.get::<NotificationHandle>() {
                handle.0.send_debug_mode_exited(DebugModeExited {
                    tool_call_id: ctx.call_id.as_str().to_owned(),
                    debug_log_path: display.clone(),
                });
            }
            display
        };

        let message = match input.note.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(note) => format!(
                "You have exited debug mode. {note}\nDebug log was at: {debug_log_path}"
            ),
            None => format!(
                "You have exited debug mode. You can now work normally. Debug log was at: {debug_log_path}"
            ),
        };

        Ok(ExitDebugModeOutput::Exited {
            message,
            debug_log_path,
        })
    }
}
