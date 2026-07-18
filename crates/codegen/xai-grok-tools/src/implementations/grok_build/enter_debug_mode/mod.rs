//! `EnterDebugMode` tool — agent-initiated entry into the debug instrumentation loop.
//!
//! On success it notifies orchestration (`DebugModeEntered`) and seeds empty
//! session debug artifacts (`debug.log`, `debug.md`) if missing. Mode reminders
//! and HITL parking live in the shell.

use crate::computer::types::AsyncFileSystem;
use crate::notification::types::DebugModeEntered;
use crate::types::output::{
    DebugFileSeedFailure, DebugFileSeedStatus, EnterDebugModeOutput, EnterDebugModeToolHints,
};
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::{
    FileSystem, NotificationHandle, resolve_debug_log_path, resolve_debug_scratch_path,
};
use crate::types::template_renderer::TemplateRenderer;
use crate::types::tool::{ToolKind, ToolNamespace};
use std::path::Path;
use std::sync::Arc;

/// Input for the `EnterDebugMode` tool (empty — entry is a binary gate).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct EnterDebugModeInput {}

/// `EnterDebugMode` tool.
#[derive(Debug, Default)]
pub struct EnterDebugModeTool;

impl crate::types::tool_metadata::ToolMetadata for EnterDebugModeTool {
    fn kind(&self) -> ToolKind {
        ToolKind::EnterDebug
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn emitted_notifications(&self) -> &'static [&'static str] {
        &["DebugModeEntered"]
    }

    fn description_template(&self) -> &str {
        r#"Use this tool when debugging a reproducible bug that needs runtime evidence. This enables debug mode: form multiple hypotheses, instrument with NDJSON logs, ask the user to reproduce, then apply a minimal fix from the logs."#
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        use crate::implementations::grok_build::exit_debug_mode::ExitDebugModeTool;
        Expr::Value(ToolRequirement::Tool {
            namespace: crate::types::tool_metadata::ToolMetadata::tool_namespace(&ExitDebugModeTool)
                .to_string(),
            id: xai_tool_runtime::Tool::id(&ExitDebugModeTool).to_string(),
            if_params: None,
        })
    }
}

impl xai_tool_runtime::Tool for EnterDebugModeTool {
    type Args = EnterDebugModeInput;
    type Output = EnterDebugModeOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("enter_debug_mode").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "enter_debug_mode",
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

    #[tracing::instrument(name = "tool.enter_debug_mode", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        _input: EnterDebugModeInput,
    ) -> Result<EnterDebugModeOutput, xai_tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;

        let (log_seed_target, scratch_seed_target, debug_log_path, debug_scratch_path, tool_hints, fs) =
            {
                let res = resources.lock().await;

                if let Some(handle) = res.get::<NotificationHandle>() {
                    handle.0.send_debug_mode_entered(DebugModeEntered {
                        tool_call_id: ctx.call_id.as_str().to_owned(),
                    });
                }

                let (log_seed_target, debug_log_path) = resolve_debug_log_path(&res);
                let (scratch_seed_target, debug_scratch_path) = resolve_debug_scratch_path(&res);

                let hints = if let Some(renderer) = res.get::<TemplateRenderer>() {
                    EnterDebugModeToolHints {
                        await_repro: renderer
                            .render("${{ tools.by_kind.debug_await_repro }}")
                            .unwrap_or_else(|_| "await_debug_reproduction".to_owned()),
                        await_verify: renderer
                            .render("${{ tools.by_kind.debug_await_verify }}")
                            .unwrap_or_else(|_| "await_debug_verification".to_owned()),
                        read_logs: renderer
                            .render("${{ tools.by_kind.debug_read_logs }}")
                            .unwrap_or_else(|_| "read_debug_logs".to_owned()),
                        exit_debug: renderer
                            .render("${{ tools.by_kind.exit_debug }}")
                            .unwrap_or_else(|_| "exit_debug_mode".to_owned()),
                        ask_user: renderer
                            .render("${{ tools.by_kind.ask_user }}")
                            .unwrap_or_else(|_| "ask_user_question".to_owned()),
                        task: renderer
                            .render("${{ tools.by_kind.task }}")
                            .unwrap_or_default(),
                    }
                } else {
                    EnterDebugModeToolHints::default()
                };

                let fs = res.get::<FileSystem>().map(|f| Arc::clone(&f.0));
                (
                    log_seed_target,
                    scratch_seed_target,
                    debug_log_path,
                    debug_scratch_path,
                    hints,
                    fs,
                )
            };

        let debug_log_seed = match (fs.as_ref(), log_seed_target.as_deref()) {
            (Some(fs), Some(target)) => probe_or_create_empty_file(fs.as_ref(), target).await,
            _ => DebugFileSeedStatus::Missing(DebugFileSeedFailure::Unavailable),
        };
        let debug_scratch_seed = match (fs.as_ref(), scratch_seed_target.as_deref()) {
            (Some(fs), Some(target)) => probe_or_create_empty_file(fs.as_ref(), target).await,
            _ => DebugFileSeedStatus::Missing(DebugFileSeedFailure::Unavailable),
        };

        tracing::info!(
            %debug_log_path,
            %debug_scratch_path,
            ?debug_log_seed,
            ?debug_scratch_seed,
            "Entered debug mode"
        );

        Ok(EnterDebugModeOutput::Entered {
            message: "You have entered debug mode. Explore the codebase, form hypotheses, \
                      instrument with NDJSON logs, and use the await tools for human reproduction \
                      and verification."
                .to_string(),
            debug_log_path,
            debug_scratch_path,
            tool_hints,
            debug_log_seed,
            debug_scratch_seed,
        })
    }
}

async fn probe_or_create_empty_file(
    fs: &dyn AsyncFileSystem,
    path: &Path,
) -> DebugFileSeedStatus {
    match fs.read_file(path).await {
        Ok(bytes) if bytes.is_empty() => DebugFileSeedStatus::Empty,
        Ok(_) => DebugFileSeedStatus::NonEmpty,
        Err(e) if e.io_error_kind() == Some(std::io::ErrorKind::NotFound) => {
            match fs.write_file(path, b"").await {
                Ok(()) => DebugFileSeedStatus::Empty,
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        path = %path.display(),
                        "Failed to create empty debug artifact"
                    );
                    DebugFileSeedStatus::Missing(DebugFileSeedFailure::NotCreated)
                }
            }
        }
        Err(e) => {
            tracing::warn!(
                error = %e,
                path = %path.display(),
                "Failed to probe debug artifact"
            );
            DebugFileSeedStatus::Missing(DebugFileSeedFailure::Unavailable)
        }
    }
}
