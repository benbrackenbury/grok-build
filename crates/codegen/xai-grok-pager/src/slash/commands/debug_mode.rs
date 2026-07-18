//! `/debug-mode` — enter Cursor-style debug mode (hypothesis + instrumentation loop).
//!
//! Distinct from `/debug` (TUI overlay toggles for scroll/FPS).

use crate::app::actions::Action;
use crate::slash::command::{CommandExecCtx, CommandResult, SlashCommand};

/// Enter debug mode.
pub struct DebugModeCommand;

impl SlashCommand for DebugModeCommand {
    fn name(&self) -> &str {
        "debug-mode"
    }

    fn description(&self) -> &str {
        "Enter debug mode (hypothesize, instrument, reproduce, fix)"
    }

    fn session_scoped(&self) -> bool {
        true
    }

    fn usage(&self) -> &str {
        "/debug-mode [bug description]"
    }

    fn takes_args(&self) -> bool {
        true
    }

    fn arg_placeholder(&self) -> Option<&str> {
        Some("[bug description]")
    }

    fn run(&self, _ctx: &mut CommandExecCtx, args: &str) -> CommandResult {
        let trimmed = args.trim();
        if trimmed.is_empty() {
            return CommandResult::Action(Action::SetDebugMode(true));
        }
        if matches!(trimmed.to_ascii_lowercase().as_str(), "off" | "exit" | "false") {
            return CommandResult::Action(Action::SetDebugMode(false));
        }
        CommandResult::Action(Action::EnterDebugMode {
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
            pager_state: PagerLocalSnapshot::default(),
        }
    }

    #[test]
    fn bare_enters_debug_mode() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut c = ctx(&models, &bundle);
        match DebugModeCommand.run(&mut c, "") {
            CommandResult::Action(Action::SetDebugMode(true)) => {}
            other => panic!("expected SetDebugMode(true), got {other:?}"),
        }
    }

    #[test]
    fn off_exits_debug_mode() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut c = ctx(&models, &bundle);
        match DebugModeCommand.run(&mut c, "off") {
            CommandResult::Action(Action::SetDebugMode(false)) => {}
            other => panic!("expected SetDebugMode(false), got {other:?}"),
        }
    }

    #[test]
    fn description_enters_with_prompt() {
        let models = ModelState::default();
        let bundle = BundleState::default();
        let mut c = ctx(&models, &bundle);
        match DebugModeCommand.run(&mut c, "cart total wrong") {
            CommandResult::Action(Action::EnterDebugMode { description }) => {
                assert_eq!(description.as_deref(), Some("cart total wrong"));
            }
            other => panic!("expected EnterDebugMode, got {other:?}"),
        }
    }
}
