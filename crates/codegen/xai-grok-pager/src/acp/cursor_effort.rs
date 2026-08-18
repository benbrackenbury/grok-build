//! Cursor ACP advertises reasoning effort as a session `thought_level`
//! config option — and only when the client sets
//! `clientCapabilities._meta.parameterizedModelPicker`.
//!
//! Without that flag, Cursor explodes each model into a single baked
//! variant (`grok-4.6[effort=high,fast=true]`) and `/effort` has nothing
//! to offer. This module stamps Grok-style `supportsReasoningEffort` /
//! `reasoningEfforts` meta onto those models so the existing `/effort`
//! and `/model <name> <effort>` UI works, and records the config option
//! id used to apply a change over `session/set_config_option`.

use std::sync::Mutex;

use agent_client_protocol as acp;
use xai_acp_lib::{AcpAgentTx, acp_send};
use xai_grok_shell::sampling::types::{
    REASONING_EFFORT_META_KEY, REASONING_EFFORTS_META_KEY, SUPPORTS_REASONING_EFFORT_META_KEY,
    parse_canonical_effort_token, reasoning_effort_meta_value, reasoning_efforts_meta_value,
};

/// Written into a model's ACP `meta` so SwitchModel can find the Cursor
/// config option id after `SessionModelState` is converted to [`ModelState`].
pub(crate) const THOUGHT_LEVEL_CONFIG_ID_META_KEY: &str = "thoughtLevelConfigId";

/// Cursor ext-method that returns each model plus its parameter config
/// options (effort, fast, …). Only registered when the parameterized
/// picker capability is advertised.
pub(crate) const CURSOR_LIST_AVAILABLE_MODELS: &str = "cursor/list_available_models";

static THOUGHT_LEVEL_CONFIG_ID: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn remember_thought_level_config_id(id: Option<String>) {
    if let Ok(mut slot) = THOUGHT_LEVEL_CONFIG_ID.lock() {
        *slot = id;
    }
}

pub(crate) fn thought_level_config_id() -> Option<String> {
    THOUGHT_LEVEL_CONFIG_ID.lock().ok().and_then(|s| s.clone())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThoughtLevelMenu {
    pub config_id: String,
    pub current: Option<String>,
    pub options: Vec<ThoughtLevelOption>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ThoughtLevelOption {
    pub value: String,
    pub label: String,
}

impl ThoughtLevelMenu {
    fn usable(&self) -> bool {
        !self.options.is_empty()
    }
}

/// Pull a thought-level menu whose values are canonical reasoning-effort
/// tokens (`low`/`medium`/`high`/`xhigh`/…). Boolean thinking toggles
/// (`true`/`false`) are ignored so `/effort` is not offered for on/off.
pub(crate) fn thought_level_menu_from_config_values(
    options: &[serde_json::Value],
) -> Option<ThoughtLevelMenu> {
    let mut menus: Vec<ThoughtLevelMenu> = options
        .iter()
        .filter_map(parse_thought_level_menu)
        .filter(ThoughtLevelMenu::usable)
        .collect();
    if menus.is_empty() {
        return None;
    }
    // Prefer an `effort` id when a model advertises both thinking + effort.
    menus.sort_by_key(|m| {
        let id = m.config_id.to_ascii_lowercase();
        (
            usize::from(id != "effort"),
            usize::from(!id.contains("effort")),
        )
    });
    Some(menus.remove(0))
}

fn parse_thought_level_menu(raw: &serde_json::Value) -> Option<ThoughtLevelMenu> {
    let obj = raw.as_object()?;
    if !is_thought_level_option(obj) {
        return None;
    }
    let config_id = obj.get("id").and_then(|v| v.as_str())?.to_string();
    // Cursor flattens `type`/`currentValue`/`options` onto the option;
    // ACP's SessionConfigOption nests them under `kind`.
    let kind = obj.get("kind").and_then(|v| v.as_object());
    let current = obj
        .get("currentValue")
        .or_else(|| kind.and_then(|k| k.get("currentValue")))
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let options = flatten_select_options(
        obj.get("options")
            .or_else(|| kind.and_then(|k| k.get("options"))),
    )
    .into_iter()
    .filter_map(|el| {
            let value = el
                .get("value")
                .and_then(|v| v.as_str())
                .or_else(|| el.as_str())?
                .to_string();
            if parse_canonical_effort_token(&value).is_none() {
                return None;
            }
            let label = el
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(&value)
                .to_string();
            Some(ThoughtLevelOption { value, label })
        })
        .collect();
    Some(ThoughtLevelMenu {
        config_id,
        current,
        options,
    })
}

fn flatten_select_options(raw: Option<&serde_json::Value>) -> Vec<serde_json::Value> {
    let Some(raw) = raw else {
        return Vec::new();
    };
    if let Some(arr) = raw.as_array() {
        return arr.clone();
    }
    if let Some(obj) = raw.as_object() {
        for key in ["options", "values", "items"] {
            if let Some(arr) = obj.get(key).and_then(|v| v.as_array()) {
                return arr.clone();
            }
        }
    }
    Vec::new()
}

fn is_thought_level_option(obj: &serde_json::Map<String, serde_json::Value>) -> bool {
    let category = obj
        .get("category")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if category == "thought_level" || category == "thoughtlevel" {
        return true;
    }
    let id = obj
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let name = obj
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    matches!(id.as_str(), "effort" | "reasoning" | "thinking" | "thought_level")
        || name.contains("effort")
        || name.contains("reasoning")
        || name.contains("thinking")
        || name.contains("thought")
}

/// Stamp `supportsReasoningEffort` / `reasoningEfforts` onto `info` from a
/// Cursor thought-level menu.
pub(crate) fn stamp_model_info_with_thought_level(
    info: &mut acp::ModelInfo,
    menu: &ThoughtLevelMenu,
) {
    let mut map = info.meta.take().unwrap_or_default();
    map.insert(
        SUPPORTS_REASONING_EFFORT_META_KEY.to_string(),
        serde_json::Value::Bool(true),
    );
    map.insert(
        THOUGHT_LEVEL_CONFIG_ID_META_KEY.to_string(),
        serde_json::Value::String(menu.config_id.clone()),
    );
    if let Some(current) = menu
        .current
        .as_deref()
        .and_then(parse_canonical_effort_token)
    {
        map.insert(
            REASONING_EFFORT_META_KEY.to_string(),
            reasoning_effort_meta_value(current),
        );
    }
    let effort_opts: Vec<_> = menu
        .options
        .iter()
        .filter_map(|opt| {
            let value = parse_canonical_effort_token(&opt.value)?;
            Some(xai_grok_shell::sampling::types::ReasoningEffortOption {
                id: opt.value.clone(),
                value,
                label: opt.label.clone(),
                description: None,
                default: menu.current.as_deref() == Some(opt.value.as_str()),
            })
        })
        .collect();
    if !effort_opts.is_empty() {
        map.insert(
            REASONING_EFFORTS_META_KEY.to_string(),
            reasoning_efforts_meta_value(&effort_opts),
        );
    }
    info.meta = Some(map);
}

/// Fetch Cursor's per-model parameter menus and stamp effort onto the
/// session catalog. Falls back to the session `configOptions` list when
/// the ext-method is missing (older cursor-agent).
pub(crate) async fn enrich_session_models(
    tx: &AcpAgentTx,
    models: Option<acp::SessionModelState>,
    config_options: Option<Vec<acp::SessionConfigOption>>,
) -> Option<acp::SessionModelState> {
    if !crate::acp::backend::current().is_cursor() {
        return apply_session_config_options(models, config_options.as_deref());
    }
    match fetch_cursor_model_list(tx).await {
        Some(body) => apply_cursor_model_list(models, &body, config_options.as_deref()),
        None => apply_session_config_options(models, config_options.as_deref()),
    }
}

async fn fetch_cursor_model_list(tx: &AcpAgentTx) -> Option<serde_json::Value> {
    let raw = serde_json::value::to_raw_value(&serde_json::json!({})).ok()?;
    let req = acp::ExtRequest::new(CURSOR_LIST_AVAILABLE_MODELS, raw.into());
    let resp = acp_send(req, tx).await.ok()?;
    serde_json::from_str(resp.0.get()).ok()
}

/// Apply session-level `configOptions` to the current catalog model.
pub(crate) fn apply_session_config_options(
    models: Option<acp::SessionModelState>,
    config_options: Option<&[acp::SessionConfigOption]>,
) -> Option<acp::SessionModelState> {
    let Some(mut state) = models else {
        return None;
    };
    let Some(options) = config_options else {
        return Some(state);
    };
    let values: Vec<serde_json::Value> = options
        .iter()
        .filter_map(|opt| serde_json::to_value(opt).ok())
        .collect();
    let Some(menu) = thought_level_menu_from_config_values(&values) else {
        return Some(state);
    };
    remember_thought_level_config_id(Some(menu.config_id.clone()));
    for info in &mut state.available_models {
        if info.model_id == state.current_model_id {
            stamp_model_info_with_thought_level(info, &menu);
        }
    }
    Some(state)
}

/// Stamp every catalog model from `cursor/list_available_models`, then
/// overlay the session's current thought-level value.
pub(crate) fn apply_cursor_model_list(
    models: Option<acp::SessionModelState>,
    list_body: &serde_json::Value,
    session_config_options: Option<&[acp::SessionConfigOption]>,
) -> Option<acp::SessionModelState> {
    let Some(mut state) = models else {
        return None;
    };
    let listed = list_body
        .get("models")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    for entry in &listed {
        let Some(id) = entry.get("value").and_then(|v| v.as_str()) else {
            continue;
        };
        let opts = entry
            .get("configOptions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let Some(menu) = thought_level_menu_from_config_values(&opts) else {
            continue;
        };
        for info in &mut state.available_models {
            if info.model_id.0.as_ref() == id {
                stamp_model_info_with_thought_level(info, &menu);
                if info.model_id == state.current_model_id {
                    remember_thought_level_config_id(Some(menu.config_id.clone()));
                }
            }
        }
    }
    apply_session_config_options(Some(state), session_config_options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn cursor_effort_option() -> serde_json::Value {
        serde_json::json!({
            "id": "effort",
            "name": "Effort",
            "category": "thought_level",
            "type": "select",
            "currentValue": "high",
            "options": [
                {"value": "low", "name": "Low"},
                {"value": "medium", "name": "Medium"},
                {"value": "high", "name": "High"},
                {"value": "xhigh", "name": "Extra High"},
            ]
        })
    }

    #[test]
    fn nested_acp_kind_shape_is_accepted() {
        let menu = thought_level_menu_from_config_values(&[serde_json::json!({
            "id": "effort",
            "name": "Effort",
            "category": "thoughtLevel",
            "kind": {
                "type": "select",
                "currentValue": "medium",
                "options": [
                    {"value": "low", "name": "Low"},
                    {"value": "medium", "name": "Medium"},
                    {"value": "high", "name": "High"},
                ]
            }
        })])
        .expect("nested kind");
        assert_eq!(menu.current.as_deref(), Some("medium"));
        assert_eq!(menu.options.len(), 3);
    }

    #[test]
    fn grok_46_effort_menu_is_selected_from_cursor_config() {
        let menu = thought_level_menu_from_config_values(&[
            serde_json::json!({
                "id": "mode",
                "category": "mode",
                "currentValue": "agent",
                "options": [{"value": "agent"}]
            }),
            cursor_effort_option(),
        ])
        .expect("effort menu");
        assert_eq!(menu.config_id, "effort");
        assert_eq!(menu.current.as_deref(), Some("high"));
        let values: Vec<_> = menu.options.iter().map(|o| o.value.as_str()).collect();
        assert_eq!(values, ["low", "medium", "high", "xhigh"]);
    }

    #[test]
    fn boolean_thinking_toggle_is_not_an_effort_menu() {
        let menu = thought_level_menu_from_config_values(&[serde_json::json!({
            "id": "thinking",
            "name": "Thinking",
            "category": "thought_level",
            "currentValue": "true",
            "options": [
                {"value": "false", "name": "Off"},
                {"value": "true", "name": "On"},
            ]
        })]);
        assert!(menu.is_none());
    }

    #[test]
    fn prefers_effort_over_boolean_thinking_when_both_present() {
        let menu = thought_level_menu_from_config_values(&[
            serde_json::json!({
                "id": "thinking",
                "category": "thought_level",
                "currentValue": "true",
                "options": [{"value": "false"}, {"value": "true"}]
            }),
            cursor_effort_option(),
        ])
        .expect("effort wins");
        assert_eq!(menu.config_id, "effort");
    }

    #[test]
    fn stamps_grok_meta_so_effort_picker_opens() {
        let id = acp::ModelId::new(Arc::from("grok-4.6"));
        let mut info = acp::ModelInfo::new(id.clone(), "grok-4.6".to_string());
        let menu = thought_level_menu_from_config_values(&[cursor_effort_option()]).unwrap();
        stamp_model_info_with_thought_level(&mut info, &menu);
        let meta = info.meta.as_ref().unwrap();
        assert_eq!(meta[SUPPORTS_REASONING_EFFORT_META_KEY], true);
        assert_eq!(meta[REASONING_EFFORT_META_KEY], "high");
        assert_eq!(meta[THOUGHT_LEVEL_CONFIG_ID_META_KEY], "effort");
        let efforts = meta[REASONING_EFFORTS_META_KEY].as_array().unwrap();
        assert_eq!(efforts.len(), 4);
    }

    #[test]
    fn apply_session_config_options_stamps_current_model_only() {
        let current = acp::ModelId::new(Arc::from("grok-4.6"));
        let other = acp::ModelId::new(Arc::from("composer-2.5"));
        let state = acp::SessionModelState::new(
            current.clone(),
            vec![
                acp::ModelInfo::new(current.clone(), "grok-4.6".to_string()),
                acp::ModelInfo::new(other.clone(), "composer-2.5".to_string()),
            ],
        );
        // Official SessionConfigOption may not accept Cursor's flat `type`
        // field — feed apply() via the list path which uses raw JSON.
        let listed = serde_json::json!({
            "models": [
                {"value": "grok-4.6", "configOptions": [cursor_effort_option()]},
                {"value": "composer-2.5", "configOptions": []},
            ]
        });
        let out = apply_cursor_model_list(Some(state), &listed, None).unwrap();
        let grok = out
            .available_models
            .iter()
            .find(|m| m.model_id == current)
            .unwrap();
        assert_eq!(
            grok.meta.as_ref().unwrap()[SUPPORTS_REASONING_EFFORT_META_KEY],
            true
        );
        let composer = out
            .available_models
            .iter()
            .find(|m| m.model_id == other)
            .unwrap();
        assert!(
            composer
                .meta
                .as_ref()
                .is_none_or(|m| m.get(SUPPORTS_REASONING_EFFORT_META_KEY).is_none())
        );

        let models = crate::acp::model_state::ModelState::from(Some(out));
        assert_eq!(
            models.resolve_effort_token("xhigh"),
            Some(xai_grok_shell::sampling::types::ReasoningEffort::Xhigh)
        );
        assert_eq!(
            models.thought_level_config_id_for(models.current.as_ref().unwrap())
                .as_deref(),
            Some("effort")
        );
    }
}
