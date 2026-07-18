//! `ReadDebugLogs` — filter/tail the session NDJSON debug log.

use crate::types::output::ReadDebugLogsOutput;
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::{FileSystem, resolve_debug_log_path};
use crate::types::tool::{ToolKind, ToolNamespace};

/// Input filters for reading debug logs.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ReadDebugLogsInput {
    /// Only events with this `runId` field.
    #[serde(default)]
    pub run_id: Option<String>,
    /// Only events whose `hypothesisId` contains this id (e.g. `A`).
    #[serde(default)]
    pub hypothesis_id: Option<String>,
    /// Max events to return (default 200, max 1000).
    #[serde(default)]
    pub max_events: Option<u32>,
    /// Only events with `timestamp` >= this value (ms).
    #[serde(default)]
    pub since_ts: Option<u64>,
}

/// `ReadDebugLogs` tool.
#[derive(Debug, Default)]
pub struct ReadDebugLogsTool;

impl crate::types::tool_metadata::ToolMetadata for ReadDebugLogsTool {
    fn kind(&self) -> ToolKind {
        ToolKind::DebugReadLogs
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        r#"Read and filter the session NDJSON debug log produced by instrumentation. Prefer this over asking the user to paste logs."#
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for ReadDebugLogsTool {
    type Args = ReadDebugLogsInput;
    type Output = ReadDebugLogsOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("read_debug_logs").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "read_debug_logs",
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

    #[tracing::instrument(name = "tool.read_debug_logs", skip_all)]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: ReadDebugLogsInput,
    ) -> Result<ReadDebugLogsOutput, xai_tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;

        let (path, display, fs) = {
            let res = resources.lock().await;
            let (abs, display) = resolve_debug_log_path(&res);
            let fs = res.get::<FileSystem>().map(|f| std::sync::Arc::clone(&f.0));
            (abs, display, fs)
        };

        let max = input.max_events.unwrap_or(200).clamp(1, 1000) as usize;
        let Some(path) = path else {
            return Ok(ReadDebugLogsOutput::Empty {
                message: format!(
                    "No absolute debug log path available (looked for {display}). \
                     Ensure debug mode is active with a session log path."
                ),
                debug_log_path: display,
                event_count: 0,
            });
        };

        let bytes = if let Some(fs) = fs.as_ref() {
            match fs.read_file(&path).await {
                Ok(b) => b,
                Err(e) if e.io_error_kind() == Some(std::io::ErrorKind::NotFound) => {
                    return Ok(ReadDebugLogsOutput::Empty {
                        message: format!(
                            "Debug log not found at {display}. Instrumentation may not have written yet."
                        ),
                        debug_log_path: display,
                        event_count: 0,
                    });
                }
                Err(e) => {
                    return Err(xai_tool_runtime::ToolError::custom(
                        "debug_log_read_failed",
                        format!("failed to read {display}: {e}"),
                    ));
                }
            }
        } else {
            match tokio::fs::read(&path).await {
                Ok(b) => b,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(ReadDebugLogsOutput::Empty {
                        message: format!(
                            "Debug log not found at {display}. Instrumentation may not have written yet."
                        ),
                        debug_log_path: display,
                        event_count: 0,
                    });
                }
                Err(e) => {
                    return Err(xai_tool_runtime::ToolError::custom(
                        "debug_log_read_failed",
                        format!("failed to read {display}: {e}"),
                    ));
                }
            }
        };

        let text = String::from_utf8_lossy(&bytes);
        let mut matched: Vec<String> = Vec::new();
        let mut total_lines = 0usize;
        let mut parse_errors = 0usize;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            total_lines += 1;
            let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
                parse_errors += 1;
                continue;
            };
            if let Some(ref run_id) = input.run_id {
                let event_run = value
                    .get("runId")
                    .or_else(|| value.get("run_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if event_run != run_id.as_str() {
                    continue;
                }
            }
            if let Some(ref hyp) = input.hypothesis_id {
                let event_hyp = value
                    .get("hypothesisId")
                    .or_else(|| value.get("hypothesis_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if !event_hyp.split(',').any(|h| h.trim() == hyp.as_str())
                    && !event_hyp.contains(hyp.as_str())
                {
                    continue;
                }
            }
            if let Some(since) = input.since_ts {
                let ts = value
                    .get("timestamp")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                if ts < since {
                    continue;
                }
            }
            matched.push(line.to_owned());
        }

        // Keep the most recent `max` matches.
        if matched.len() > max {
            let skip = matched.len() - max;
            matched = matched.split_off(skip);
        }

        if matched.is_empty() {
            return Ok(ReadDebugLogsOutput::Empty {
                message: format!(
                    "No matching events in {display} (scanned {total_lines} lines, \
                     parse_errors={parse_errors}). Filters: run_id={:?}, hypothesis_id={:?}, since_ts={:?}",
                    input.run_id, input.hypothesis_id, input.since_ts
                ),
                debug_log_path: display,
                event_count: 0,
            });
        }

        let event_count = matched.len();
        let body = matched.join("\n");
        Ok(ReadDebugLogsOutput::Events {
            message: format!(
                "Showing {event_count} event(s) from {display} \
                 (scanned {total_lines} lines, parse_errors={parse_errors})."
            ),
            debug_log_path: display,
            event_count,
            events_ndjson: body,
        })
    }
}
