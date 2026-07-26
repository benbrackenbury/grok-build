//! Ask mode state machine and prompt text generation.
//!
//! Mirrors [`super::plan_mode::PlanModeTracker`] / [`super::debug_mode::DebugModeTracker`]
//! for lifecycle. Ask mode is Cursor-style read-only Q&A: the agent may explore
//! with read/search tools, but **all file edits are rejected** (stricter than
//! plan mode, which allows `plan.md`).
use std::path::PathBuf;

/// Lifecycle of ask mode on the SessionActor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AskModeState {
    /// Not in ask mode.
    Inactive,
    /// User toggled ask on; model has not been told yet.
    Pending,
    /// Ask mode active; model has the read-only playbook.
    Active,
    /// User toggled off mid-turn; exit after the turn completes.
    ExitPending,
}

/// Tracks the full ask mode lifecycle for a session.
pub struct AskModeTracker {
    state: AskModeState,
    was_previously_active: bool,
    reminder_count: u32,
    pending_exit_reminder: bool,
    /// Buffered mid-turn activation reminder text.
    pending_activation: Option<PendingActivation>,
    /// Session directory (used only for snapshot restore symmetry with plan/debug).
    #[allow(dead_code)]
    session_dir: PathBuf,
}

struct PendingActivation {
    text: String,
    prior_was_previously_active: bool,
}

/// Serializable snapshot of ask mode lifecycle state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AskModeSnapshot {
    pub state: AskModeState,
    pub was_previously_active: bool,
    pub reminder_count: u32,
    pub pending_exit_reminder: bool,
}

impl AskModeTracker {
    pub fn new(session_dir: PathBuf) -> Self {
        Self {
            state: AskModeState::Inactive,
            was_previously_active: false,
            reminder_count: 0,
            pending_exit_reminder: false,
            pending_activation: None,
            session_dir,
        }
    }

    pub fn from_snapshot(session_dir: PathBuf, mut snapshot: AskModeSnapshot) -> Self {
        match snapshot.state {
            AskModeState::Pending => {
                snapshot.state = AskModeState::Inactive;
            }
            AskModeState::ExitPending => {
                snapshot.state = AskModeState::Inactive;
                snapshot.pending_exit_reminder = true;
            }
            _ => {}
        }
        Self {
            state: snapshot.state,
            was_previously_active: snapshot.was_previously_active,
            reminder_count: snapshot.reminder_count,
            pending_exit_reminder: snapshot.pending_exit_reminder,
            pending_activation: None,
            session_dir,
        }
    }

    pub fn snapshot(&self) -> AskModeSnapshot {
        AskModeSnapshot {
            state: self.state,
            was_previously_active: self.was_previously_active,
            reminder_count: self.reminder_count,
            pending_exit_reminder: self.pending_exit_reminder,
        }
    }

    pub fn state(&self) -> AskModeState {
        self.state
    }

    pub fn is_active(&self) -> bool {
        self.state == AskModeState::Active
    }

    pub fn is_reentry(&self) -> bool {
        self.was_previously_active && self.state == AskModeState::Pending
    }

    pub fn should_use_full_reminder(&self) -> bool {
        self.reminder_count.is_multiple_of(2)
    }

    pub fn has_pending_exit_reminder(&self) -> bool {
        self.pending_exit_reminder
    }

    pub fn enter_pending(&mut self) -> bool {
        match self.state {
            AskModeState::Inactive => {
                self.state = AskModeState::Pending;
                self.pending_exit_reminder = false;
                true
            }
            AskModeState::ExitPending => {
                self.state = AskModeState::Active;
                self.pending_exit_reminder = false;
                true
            }
            _ => false,
        }
    }

    pub fn activate(&mut self) -> bool {
        if self.state != AskModeState::Pending {
            return false;
        }
        self.state = AskModeState::Active;
        self.was_previously_active = true;
        self.reminder_count = 0;
        true
    }

    pub fn activate_mid_turn(&mut self, rendered_reminder: String) -> bool {
        if self.state != AskModeState::Pending {
            return false;
        }
        let prior_was_previously_active = self.was_previously_active;
        self.state = AskModeState::Active;
        self.was_previously_active = true;
        self.reminder_count = 0;
        self.pending_activation = Some(PendingActivation {
            text: rendered_reminder,
            prior_was_previously_active,
        });
        true
    }

    pub fn take_pending_activation(&mut self) -> Option<String> {
        self.pending_activation.take().map(|p| p.text)
    }

    pub fn has_pending_activation(&self) -> bool {
        self.pending_activation.is_some()
    }

    pub fn user_exit(&mut self, turn_in_flight: bool) {
        if let Some(pending) = self.pending_activation.take()
            && self.state == AskModeState::Active
        {
            self.state = AskModeState::Inactive;
            self.was_previously_active = pending.prior_was_previously_active;
            return;
        }
        match self.state {
            AskModeState::Pending => {
                self.state = AskModeState::Inactive;
            }
            AskModeState::Active => {
                if turn_in_flight {
                    self.state = AskModeState::ExitPending;
                } else {
                    self.state = AskModeState::Inactive;
                    self.pending_exit_reminder = true;
                }
            }
            _ => {}
        }
    }

    pub fn complete_deferred_exit(&mut self) {
        if self.state != AskModeState::ExitPending {
            return;
        }
        self.state = AskModeState::Inactive;
        self.pending_exit_reminder = true;
    }

    pub fn record_reminder_injected(&mut self) {
        self.reminder_count += 1;
    }

    pub fn clear_pending_exit_reminder(&mut self) {
        self.pending_exit_reminder = false;
    }

    pub fn reset_after_compaction(&mut self) {
        if self.state == AskModeState::Active {
            self.reminder_count = 0;
            self.pending_activation = None;
        }
    }
}

/// Full ask-mode reminder (read-only Q&A playbook).
pub fn ask_mode_reminder_full_template() -> &'static str {
    "\
Ask mode is active. Answer the user's questions by exploring the codebase — do not make any edits or writes.

## Rules
- You MAY use read-only tools: read files, search/grep, list directories, web search/fetch, and ask clarifying questions.
- You MUST NOT edit, write, create, delete, or apply patches to any files.
- Prefer answering from evidence in the repo. Quote paths and symbols when helpful.
- If the user asks you to implement a change, explain the approach and suggest switching to Agent (or Plan) mode — do not attempt the edit yourself.
- Shell/MCP tools that mutate the system are out of scope; stick to inspection."
}

pub fn ask_mode_reminder_sparse_template() -> &'static str {
    "Ask mode is still active. Explore and answer questions only — no file edits or writes."
}

pub fn ask_mode_reentry_reminder_template() -> &'static str {
    "\
## Returning to Ask Mode

You are entering ask mode again. Continue answering questions with read-only exploration. Do not edit files."
}

pub fn ask_mode_exit_reminder_template() -> &'static str {
    "Ask mode has been turned off. You may edit files and use the full toolset again, subject to normal permissions."
}

pub fn ask_mode_edit_rejected_template() -> &'static str {
    "Rejected: file edits are not allowed in ask mode. Switch to Agent mode (or Plan mode) to make changes."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tracker() -> AskModeTracker {
        AskModeTracker::new(PathBuf::from("/tmp/test-ask-session"))
    }

    #[test]
    fn user_initiated_lifecycle() {
        let mut t = test_tracker();
        assert_eq!(t.state(), AskModeState::Inactive);
        assert!(t.enter_pending());
        assert_eq!(t.state(), AskModeState::Pending);
        assert!(t.activate());
        assert_eq!(t.state(), AskModeState::Active);
        t.user_exit(false);
        assert_eq!(t.state(), AskModeState::Inactive);
        assert!(t.has_pending_exit_reminder());
    }

    #[test]
    fn user_exit_while_turn_in_flight() {
        let mut t = test_tracker();
        t.enter_pending();
        t.activate();
        t.user_exit(true);
        assert_eq!(t.state(), AskModeState::ExitPending);
        t.complete_deferred_exit();
        assert_eq!(t.state(), AskModeState::Inactive);
        assert!(t.has_pending_exit_reminder());
    }

    #[test]
    fn pending_cancel_is_clean() {
        let mut t = test_tracker();
        t.enter_pending();
        t.user_exit(false);
        assert_eq!(t.state(), AskModeState::Inactive);
        assert!(!t.has_pending_exit_reminder());
    }

    #[test]
    fn reentry_detected() {
        let mut t = test_tracker();
        t.enter_pending();
        t.activate();
        t.user_exit(false);
        t.clear_pending_exit_reminder();
        t.enter_pending();
        assert!(t.is_reentry());
    }

    #[test]
    fn reminder_alternation() {
        let mut t = test_tracker();
        t.enter_pending();
        t.activate();
        assert!(t.should_use_full_reminder());
        t.record_reminder_injected();
        assert!(!t.should_use_full_reminder());
        t.record_reminder_injected();
        assert!(t.should_use_full_reminder());
    }

    #[test]
    fn midturn_activation_buffers_and_delivers_exactly_once() {
        let mut t = test_tracker();
        t.enter_pending();
        assert!(t.activate_mid_turn("reminder text".into()));
        assert_eq!(t.state(), AskModeState::Active);
        assert!(t.has_pending_activation());
        assert_eq!(
            t.take_pending_activation().as_deref(),
            Some("reminder text")
        );
        assert!(!t.has_pending_activation());
        assert_eq!(t.take_pending_activation(), None);
    }

    #[test]
    fn snapshot_round_trip_collapses_pending() {
        let mut t = test_tracker();
        t.enter_pending();
        let snap = t.snapshot();
        let restored = AskModeTracker::from_snapshot(PathBuf::from("/tmp/x"), snap);
        assert_eq!(restored.state(), AskModeState::Inactive);
    }
}
