//! `/ask` — enter Cursor-style ask mode (read-only Q&A / exploration).

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Enter ask mode.
pub struct AskCommand;

impl SlashCommand for AskCommand {
    fn name(&self) -> &str {
        "ask"
    }

    fn description(&self) -> &str {
        "Enter ask mode (read-only Q&A; explore without edits)"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn usage(&self) -> &str {
        "/ask [question]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[question]")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return CommandResult::Action(Action::SetAskMode(true));
        }
        if matches!(trimmed.to_ascii_lowercase().as_str(), "off" | "exit" | "false") {
            return CommandResult::Action(Action::SetAskMode(false));
        }
        CommandResult::Action(Action::EnterAskMode {
            description: Some(trimmed.to_string()),
        })
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
    fn bare_enters_ask_mode() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut c = ctx(&models, &bundle);
        match AskCommand.run(&mut c, "") {
            CommandResult::Action(Action::SetAskMode(true)) => {}
            other => panic!("expected SetAskMode(true), got {other:?}"),
        }
    }

    #[test]
    fn off_exits_ask_mode() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut c = ctx(&models, &bundle);
        match AskCommand.run(&mut c, "off") {
            CommandResult::Action(Action::SetAskMode(false)) => {}
            other => panic!("expected SetAskMode(false), got {other:?}"),
        }
    }

    #[test]
    fn description_enters_with_prompt() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut c = ctx(&models, &bundle);
        match AskCommand.run(&mut c, "how does auth work?") {
            CommandResult::Action(Action::EnterAskMode { description }) => {
                assert_eq!(description.as_deref(), Some("how does auth work?"));
            }
            other => panic!("expected EnterAskMode, got {other:?}"),
        }
    }
}
