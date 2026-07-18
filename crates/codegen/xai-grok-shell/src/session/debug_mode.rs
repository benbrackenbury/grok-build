//! Debug mode state machine and prompt text generation.
//!
//! Mirrors [`super::plan_mode::PlanModeTracker`] for lifecycle, but **allows
//! edits** — the agent must instrument source and apply fixes. Enforcement is
//! playbook + HITL gates (`await_debug_reproduction` /
//! `await_debug_verification`), not an edit gate.
use std::path::{Path, PathBuf};

/// Lifecycle of debug mode on the SessionActor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DebugModeState {
    /// Not in debug mode.
    Inactive,
    /// User toggled debug on; model has not been told yet.
    Pending,
    /// Debug mode active; model has the playbook.
    Active,
    /// User toggled off mid-turn; exit after the turn completes.
    ExitPending,
}

/// Sub-phase while lifecycle is [`DebugModeState::Active`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugPhase {
    /// Exploring code / forming hypotheses / instrumenting.
    #[default]
    Exploring,
    /// Waiting for the user to reproduce (`await_debug_reproduction` parked).
    AwaitingReproduction,
    /// User proceeded; agent is analyzing logs / fixing.
    Analyzing,
    /// Waiting for Mark Fixed / Still broken.
    AwaitingVerification,
    /// User marked fixed; agent is stripping instrumentation.
    CleaningUp,
}

/// Tracks the full debug mode lifecycle for a session.
pub struct DebugModeTracker {
    state: DebugModeState,
    phase: DebugPhase,
    was_previously_active: bool,
    reminder_count: u32,
    pending_exit_reminder: bool,
    /// HITL: reproduction chrome outstanding.
    awaiting_reproduction: bool,
    /// HITL: verification chrome outstanding.
    awaiting_verification: bool,
    /// Absolute path to session NDJSON log: `<session_dir>/debug.log`.
    debug_log_path: PathBuf,
    /// Absolute path to scratch markdown: `<session_dir>/debug.md`.
    debug_scratch_path: PathBuf,
    /// Buffered mid-turn activation reminder text.
    pending_activation: Option<PendingActivation>,
}

struct PendingActivation {
    text: String,
    prior_was_previously_active: bool,
}

/// Serializable snapshot of debug mode lifecycle state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DebugModeSnapshot {
    pub state: DebugModeState,
    pub phase: DebugPhase,
    pub was_previously_active: bool,
    pub reminder_count: u32,
    pub pending_exit_reminder: bool,
    #[serde(default)]
    pub awaiting_reproduction: bool,
    #[serde(default)]
    pub awaiting_verification: bool,
}

impl DebugModeTracker {
    pub fn new(session_dir: PathBuf) -> Self {
        Self {
            state: DebugModeState::Inactive,
            phase: DebugPhase::Exploring,
            was_previously_active: false,
            reminder_count: 0,
            pending_exit_reminder: false,
            awaiting_reproduction: false,
            awaiting_verification: false,
            debug_log_path: session_dir.join("debug.log"),
            debug_scratch_path: session_dir.join("debug.md"),
            pending_activation: None,
        }
    }

    pub fn from_snapshot(session_dir: PathBuf, mut snapshot: DebugModeSnapshot) -> Self {
        match snapshot.state {
            DebugModeState::Pending => {
                snapshot.state = DebugModeState::Inactive;
            }
            DebugModeState::ExitPending => {
                snapshot.state = DebugModeState::Inactive;
                snapshot.pending_exit_reminder = true;
            }
            _ => {}
        }
        Self {
            state: snapshot.state,
            phase: snapshot.phase,
            was_previously_active: snapshot.was_previously_active,
            reminder_count: snapshot.reminder_count,
            pending_exit_reminder: snapshot.pending_exit_reminder,
            awaiting_reproduction: snapshot.awaiting_reproduction,
            awaiting_verification: snapshot.awaiting_verification,
            debug_log_path: session_dir.join("debug.log"),
            debug_scratch_path: session_dir.join("debug.md"),
            pending_activation: None,
        }
    }

    pub fn snapshot(&self) -> DebugModeSnapshot {
        DebugModeSnapshot {
            state: self.state,
            phase: self.phase,
            was_previously_active: self.was_previously_active,
            reminder_count: self.reminder_count,
            pending_exit_reminder: self.pending_exit_reminder,
            awaiting_reproduction: self.awaiting_reproduction,
            awaiting_verification: self.awaiting_verification,
        }
    }

    pub fn state(&self) -> DebugModeState {
        self.state
    }

    pub fn phase(&self) -> DebugPhase {
        self.phase
    }

    pub fn is_active(&self) -> bool {
        self.state == DebugModeState::Active
    }

    pub fn debug_log_path(&self) -> &Path {
        &self.debug_log_path
    }

    pub fn debug_scratch_path(&self) -> &Path {
        &self.debug_scratch_path
    }

    pub fn set_awaiting_reproduction(&mut self, awaiting: bool) {
        self.awaiting_reproduction = awaiting;
        if awaiting {
            self.phase = DebugPhase::AwaitingReproduction;
            self.awaiting_verification = false;
        }
    }

    pub fn set_awaiting_verification(&mut self, awaiting: bool) {
        self.awaiting_verification = awaiting;
        if awaiting {
            self.phase = DebugPhase::AwaitingVerification;
            self.awaiting_reproduction = false;
        }
    }

    pub fn is_awaiting_reproduction(&self) -> bool {
        self.awaiting_reproduction
    }

    pub fn is_awaiting_verification(&self) -> bool {
        self.awaiting_verification
    }

    pub fn set_phase(&mut self, phase: DebugPhase) {
        self.phase = phase;
    }

    pub fn should_use_full_reminder(&self) -> bool {
        self.reminder_count.is_multiple_of(2)
    }

    pub fn has_pending_exit_reminder(&self) -> bool {
        self.pending_exit_reminder
    }

    pub fn enter_pending(&mut self) -> bool {
        match self.state {
            DebugModeState::Inactive => {
                self.state = DebugModeState::Pending;
                self.pending_exit_reminder = false;
                self.phase = DebugPhase::Exploring;
                true
            }
            DebugModeState::ExitPending => {
                self.state = DebugModeState::Active;
                self.pending_exit_reminder = false;
                true
            }
            _ => false,
        }
    }

    pub fn activate(&mut self) -> bool {
        if self.state != DebugModeState::Pending {
            return false;
        }
        self.state = DebugModeState::Active;
        self.was_previously_active = true;
        self.reminder_count = 0;
        self.phase = DebugPhase::Exploring;
        true
    }

    pub fn activate_mid_turn(&mut self, rendered_reminder: String) -> bool {
        if self.state != DebugModeState::Pending {
            return false;
        }
        let prior_was_previously_active = self.was_previously_active;
        self.state = DebugModeState::Active;
        self.was_previously_active = true;
        self.reminder_count = 0;
        self.phase = DebugPhase::Exploring;
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

    pub fn activate_from_tool(&mut self) -> bool {
        if self.state != DebugModeState::Inactive {
            return false;
        }
        self.state = DebugModeState::Active;
        self.was_previously_active = true;
        self.reminder_count = 0;
        self.pending_exit_reminder = false;
        self.phase = DebugPhase::Exploring;
        true
    }

    pub fn deactivate_approved(&mut self) -> bool {
        if self.state != DebugModeState::Active {
            return false;
        }
        self.state = DebugModeState::Inactive;
        self.reminder_count = 0;
        self.awaiting_reproduction = false;
        self.awaiting_verification = false;
        self.phase = DebugPhase::Exploring;
        self.pending_activation = None;
        true
    }

    pub fn user_exit(&mut self, turn_in_flight: bool) {
        self.awaiting_reproduction = false;
        self.awaiting_verification = false;
        if let Some(pending) = self.pending_activation.take()
            && self.state == DebugModeState::Active
        {
            self.state = DebugModeState::Inactive;
            self.was_previously_active = pending.prior_was_previously_active;
            self.phase = DebugPhase::Exploring;
            return;
        }
        match self.state {
            DebugModeState::Pending => {
                self.state = DebugModeState::Inactive;
                self.phase = DebugPhase::Exploring;
            }
            DebugModeState::Active => {
                if turn_in_flight {
                    self.state = DebugModeState::ExitPending;
                } else {
                    self.state = DebugModeState::Inactive;
                    self.pending_exit_reminder = true;
                    self.phase = DebugPhase::Exploring;
                }
            }
            _ => {}
        }
    }

    pub fn complete_deferred_exit(&mut self) {
        if self.state != DebugModeState::ExitPending {
            return;
        }
        self.state = DebugModeState::Inactive;
        self.pending_exit_reminder = true;
        self.phase = DebugPhase::Exploring;
        self.awaiting_reproduction = false;
        self.awaiting_verification = false;
    }

    pub fn queue_exit_reminder(&mut self) {
        self.pending_exit_reminder = true;
    }

    pub fn record_reminder_injected(&mut self) {
        self.reminder_count += 1;
    }

    pub fn clear_pending_exit_reminder(&mut self) {
        self.pending_exit_reminder = false;
    }

    pub fn reset_after_compaction(&mut self) {
        if self.state == DebugModeState::Active {
            self.reminder_count = 0;
            self.pending_activation = None;
        }
    }
}

/// Full debug mode reminder (playbook + log path).
///
/// Render with `TemplateRenderer::render_with_extra` and:
/// `{ "debug_log_path": "...", "debug_scratch_path": "..." }`.
pub fn debug_mode_reminder_full_template() -> &'static str {
    "\
Debug mode is active. Do not jump to a speculative large fix — gather runtime evidence first.

## Artifacts
- NDJSON debug log (append one JSON object per line): `${{ debug_log_path }}`
- Scratch notes (hypotheses table, findings): `${{ debug_scratch_path }}`

## Loop
1. Explore relevant code (read/search; optional explore subagents).
2. Write 3–5 labeled hypotheses (A, B, C…) into the scratch file.
3. Instrument with tagged regions (`// #region agent log` / `# region agent log`) that append NDJSON events including `hypothesisId`, `runId`, `location`, `message`, `data`, `timestamp`.
4. Call `${{ tools.by_kind.debug_await_repro }}` with concrete reproduction steps (include rebuild/restart if needed).
5. After the user proceeds: read the log (`${{ tools.by_kind.debug_read_logs }}` or read the log file), mark each hypothesis CONFIRMED/REJECTED/INCONCLUSIVE, apply a **minimal** fix. Keep instrumentation.
6. Call `${{ tools.by_kind.debug_await_verify }}`.
7. On Mark Fixed: remove every agent-log region, then call `${{ tools.by_kind.exit_debug }}`.
8. On Still broken: add/refine instrumentation and repeat from step 4 with a new `runId`.

Fail soft on log IO (never crash the app under test). Prefer evidence over large rewrites."
}

pub fn debug_mode_reminder_sparse_template() -> &'static str {
    "Debug mode is still active. Prefer runtime evidence over large speculative fixes. Use the await-reproduction / await-verification tools for human gates."
}

pub fn debug_mode_reentry_reminder_template() -> &'static str {
    "\
## Returning to Debug Mode

You are entering debug mode again. Prior log/scratch may exist at `${{ debug_log_path }}` and `${{ debug_scratch_path }}`. Continue the hypothesis → instrument → reproduce → verify loop; clean up instrumentation before exiting."
}

pub fn debug_mode_exit_reminder_template() -> &'static str {
    "\
You have exited debug mode. You can work normally. If agent log regions (`#region agent log`) remain in the workspace, remove them before committing."
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_tracker() -> DebugModeTracker {
        DebugModeTracker::new(PathBuf::from("/tmp/test-debug-session"))
    }

    #[test]
    fn user_initiated_lifecycle() {
        let mut t = test_tracker();
        assert_eq!(t.state(), DebugModeState::Inactive);
        assert!(t.enter_pending());
        assert_eq!(t.state(), DebugModeState::Pending);
        assert!(t.activate());
        assert_eq!(t.state(), DebugModeState::Active);
        assert_eq!(t.phase(), DebugPhase::Exploring);
        assert!(t.deactivate_approved());
        assert_eq!(t.state(), DebugModeState::Inactive);
    }

    #[test]
    fn tool_entry_skips_pending() {
        let mut t = test_tracker();
        assert!(t.activate_from_tool());
        assert_eq!(t.state(), DebugModeState::Active);
        assert!(!t.activate_from_tool());
    }

    #[test]
    fn awaiting_flags_set_phase() {
        let mut t = test_tracker();
        t.activate_from_tool();
        t.set_awaiting_reproduction(true);
        assert!(t.is_awaiting_reproduction());
        assert_eq!(t.phase(), DebugPhase::AwaitingReproduction);
        t.set_awaiting_reproduction(false);
        t.set_awaiting_verification(true);
        assert!(t.is_awaiting_verification());
        assert!(!t.is_awaiting_reproduction());
        assert_eq!(t.phase(), DebugPhase::AwaitingVerification);
    }

    #[test]
    fn paths_under_session_dir() {
        let t = DebugModeTracker::new(PathBuf::from("/tmp/sess-xyz"));
        assert_eq!(
            t.debug_log_path(),
            Path::new("/tmp/sess-xyz/debug.log")
        );
        assert_eq!(
            t.debug_scratch_path(),
            Path::new("/tmp/sess-xyz/debug.md")
        );
    }

    #[test]
    fn snapshot_collapse_pending() {
        let snap = DebugModeSnapshot {
            state: DebugModeState::Pending,
            phase: DebugPhase::Exploring,
            was_previously_active: false,
            reminder_count: 0,
            pending_exit_reminder: false,
            awaiting_reproduction: false,
            awaiting_verification: false,
        };
        let t = DebugModeTracker::from_snapshot(PathBuf::from("/tmp/s"), snap);
        assert_eq!(t.state(), DebugModeState::Inactive);
    }

    #[test]
    fn user_exit_mid_turn_defers() {
        let mut t = test_tracker();
        t.activate_from_tool();
        t.user_exit(true);
        assert_eq!(t.state(), DebugModeState::ExitPending);
        t.complete_deferred_exit();
        assert_eq!(t.state(), DebugModeState::Inactive);
        assert!(t.has_pending_exit_reminder());
    }
}
