//! `/backend`, `/cursor`, `/grok` — switch the ACP agent backend.

use crate::acp::backend::AgentBackend;
use crate::app::actions::Action;
use crate::slash::command::{AppCtx, ArgItem, CommandExecCtx, CommandResult, SlashCommand};

fn status_message() -> String {
    let current = crate::acp::backend::current();
    match current {
        AgentBackend::Grok => {
            "Backend: Grok Build (SpaceXAI). Use /cursor to run on your Cursor subscription."
                .into()
        }
        AgentBackend::Cursor => {
            "Backend: Cursor (local cursor-agent login). Use /grok to switch back to Grok Build."
                .into()
        }
    }
}

/// `/backend [grok|cursor]` — show or switch the ACP backend.
pub struct BackendCommand;

impl SlashCommand for BackendCommand {
    fn name(&self) -> &str {
        "backend"
    }

    fn aliases(&self) -> &[&str] {
        &["provider"]
    }

    fn description(&self) -> &str {
        "Switch between Grok Build and Cursor (subscription)"
    }

    fn usage(&self) -> &str {
        "/backend [grok|cursor]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[grok|cursor]")
    }

    fn suggest_args(&self, _ctx: &AppCtx, args_query: &str) -> Option<Vec<ArgItem>> {
        let q = args_query.trim().to_ascii_lowercase();
        let items = [
            ("grok", "SpaceXAI / Grok Build"),
            ("cursor", "Cursor Agent (existing login)"),
        ];
        let filtered: Vec<ArgItem> = items
            .into_iter()
            .filter(|(name, _)| q.is_empty() || name.starts_with(&q))
            .map(|(name, desc)| ArgItem {
                display: name.to_string(),
                match_text: name.to_string(),
                insert_text: name.to_string(),
                description: desc.to_string(),
            })
            .collect();
        if filtered.is_empty() {
            None
        } else {
            Some(filtered)
        }
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return CommandResult::Message(status_message());
        }
        match AgentBackend::parse_name(trimmed) {
            Some(backend) => CommandResult::Action(Action::SwitchAcpBackend(backend)),
            None => CommandResult::Error(format!(
                "Unknown backend '{trimmed}'. Use grok or cursor."
            )),
        }
    }
}

/// `/cursor` — switch to the local Cursor Agent subscription.
pub struct CursorCommand;

impl SlashCommand for CursorCommand {
    fn name(&self) -> &str {
        "cursor"
    }

    fn description(&self) -> &str {
        "Use Cursor Agent (existing machine login)"
    }

    fn usage(&self) -> &str {
        "/cursor"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        if !args.trim().is_empty() {
            return CommandResult::Error("/cursor does not take arguments".into());
        }
        CommandResult::Action(Action::SwitchAcpBackend(AgentBackend::Cursor))
    }
}

/// `/grok` — switch back to the in-process Grok / SpaceXAI agent.
pub struct GrokCommand;

impl SlashCommand for GrokCommand {
    fn name(&self) -> &str {
        "grok"
    }

    fn aliases(&self) -> &[&str] {
        &["spacex"]
    }

    fn description(&self) -> &str {
        "Use Grok Build (SpaceXAI)"
    }

    fn usage(&self) -> &str {
        "/grok"
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        if !args.trim().is_empty() {
            return CommandResult::Error("/grok does not take arguments".into());
        }
        CommandResult::Action(Action::SwitchAcpBackend(AgentBackend::Grok))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::model_state::ModelState;
    use crate::app::bundle::BundleState;
    use crate::settings::PagerLocalSnapshot;

    fn ctx<'a>(models: &'a ModelState, bundle: &'a BundleState) -> CommandExecCtx<'a> {
        CommandExecCtx {
            models,
            session_id: None,
            bundle_state: bundle,
            screen_mode: crate::app::ScreenMode::Inline,
            billing_surface_visible: true,
            usage_command_visible: true,
            pager_state: PagerLocalSnapshot::default(),
        }
    }

    #[test]
    fn bare_backend_is_status() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut c = ctx(&models, &bundle);
        match BackendCommand.run(&mut c, "") {
            CommandResult::Message(msg) => assert!(msg.contains("Backend:")),
            other => panic!("expected Message, got {other:?}"),
        }
    }

    #[test]
    fn backend_cursor_dispatches() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut c = ctx(&models, &bundle);
        match BackendCommand.run(&mut c, "cursor") {
            CommandResult::Action(Action::SwitchAcpBackend(AgentBackend::Cursor)) => {}
            other => panic!("expected SwitchAcpBackend(Cursor), got {other:?}"),
        }
    }

    #[test]
    fn cursor_command_dispatches() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut c = ctx(&models, &bundle);
        match CursorCommand.run(&mut c, "") {
            CommandResult::Action(Action::SwitchAcpBackend(AgentBackend::Cursor)) => {}
            other => panic!("expected SwitchAcpBackend(Cursor), got {other:?}"),
        }
    }
}
