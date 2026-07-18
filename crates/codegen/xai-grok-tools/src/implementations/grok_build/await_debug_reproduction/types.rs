//! ACP reverse-request types for `await_debug_reproduction`.

use serde::{Deserialize, Serialize};

/// Agent → client reverse-request payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwaitDebugReproductionExtRequest {
    pub session_id: String,
    pub tool_call_id: String,
    pub steps: String,
    pub run_id: String,
    pub log_path: String,
}

/// Client → agent response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwaitDebugReproductionExtResponse {
    /// `proceeded` | `cancelled` | `abandoned`
    pub outcome: String,
    #[serde(default)]
    pub notes: Option<String>,
}
