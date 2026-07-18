//! Compact debug-mode HITL chrome: Proceed / Mark Fixed action bar.

use agent_client_protocol as acp;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use xai_acp_lib::AcpResult;
use xai_grok_tools::implementations::grok_build::await_debug_reproduction::types::{
    AwaitDebugReproductionExtRequest, AwaitDebugReproductionExtResponse,
};
use xai_grok_tools::implementations::grok_build::await_debug_verification::types::{
    AwaitDebugVerificationExtRequest, AwaitDebugVerificationExtResponse,
};

use crate::theme::Theme;
use crate::views::prompt_widget::StashedPrompt;

/// Which debug HITL gate is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugHitlKind {
    Reproduction,
    Verification,
}

/// Parked reverse-request state for debug Proceed / Mark Fixed.
pub struct DebugHitlViewState {
    pub kind: DebugHitlKind,
    pub tool_call_id: String,
    pub session_id: String,
    /// Steps (repro) or fix summary (verify).
    pub body: String,
    pub run_id: String,
    pub log_path: String,
    /// Taken via [`Self::take_stashed_prompt`] on dismiss so we don't move
    /// out of a type that implements `Drop`.
    stashed_prompt: Option<StashedPrompt>,
    response_tx: Option<tokio::sync::oneshot::Sender<AcpResult<acp::ExtResponse>>>,
}

impl DebugHitlViewState {
    pub fn from_reproduction(
        request: AwaitDebugReproductionExtRequest,
        stashed_prompt: StashedPrompt,
        response_tx: tokio::sync::oneshot::Sender<AcpResult<acp::ExtResponse>>,
    ) -> Self {
        Self {
            kind: DebugHitlKind::Reproduction,
            tool_call_id: request.tool_call_id,
            session_id: request.session_id,
            body: request.steps,
            run_id: request.run_id,
            log_path: request.log_path,
            stashed_prompt: Some(stashed_prompt),
            response_tx: Some(response_tx),
        }
    }

    pub fn from_verification(
        request: AwaitDebugVerificationExtRequest,
        stashed_prompt: StashedPrompt,
        response_tx: tokio::sync::oneshot::Sender<AcpResult<acp::ExtResponse>>,
    ) -> Self {
        Self {
            kind: DebugHitlKind::Verification,
            tool_call_id: request.tool_call_id,
            session_id: request.session_id,
            body: request.summary,
            run_id: request.run_id,
            log_path: request.log_path,
            stashed_prompt: Some(stashed_prompt),
            response_tx: Some(response_tx),
        }
    }

    /// Take the stashed composer so the caller can restore it without
    /// moving out of a `Drop` type.
    pub fn take_stashed_prompt(&mut self) -> Option<StashedPrompt> {
        self.stashed_prompt.take()
    }

    fn send_raw(&mut self, value: impl serde::Serialize) {
        if let Some(tx) = self.response_tx.take() {
            let raw = serde_json::value::to_raw_value(&value)
                .expect("debug HITL response serialization");
            let _ = tx.send(Ok(acp::ExtResponse::new(raw.into())));
        }
    }

    pub fn send_reproduction_outcome(&mut self, outcome: &str, notes: Option<String>) {
        let resp = AwaitDebugReproductionExtResponse {
            outcome: outcome.to_owned(),
            notes,
        };
        self.send_raw(resp);
    }

    pub fn send_verification_outcome(&mut self, outcome: &str, notes: Option<String>) {
        let resp = AwaitDebugVerificationExtResponse {
            outcome: outcome.to_owned(),
            notes,
        };
        self.send_raw(resp);
    }

    pub fn send_stale_cancel(&mut self) {
        match self.kind {
            DebugHitlKind::Reproduction => self.send_reproduction_outcome("cancelled", None),
            DebugHitlKind::Verification => self.send_verification_outcome("cancelled", None),
        }
    }

    /// One-line controls strip for the prompt chrome.
    pub fn controls_line(&self) -> Line<'static> {
        let theme = Theme::current();
        let dim = Style::default().fg(theme.md_muted);
        let key = Style::default()
            .fg(theme.accent_system)
            .add_modifier(Modifier::BOLD);
        match self.kind {
            DebugHitlKind::Reproduction => Line::from(vec![
                Span::styled("Debug repro  ", dim),
                Span::styled("p", key),
                Span::styled(" Proceed  ", dim),
                Span::styled("q", key),
                Span::styled(" Abandon", dim),
            ]),
            DebugHitlKind::Verification => Line::from(vec![
                Span::styled("Debug verify  ", dim),
                Span::styled("f", key),
                Span::styled(" Mark Fixed  ", dim),
                Span::styled("b", key),
                Span::styled(" Still broken  ", dim),
                Span::styled("q", key),
                Span::styled(" Abandon", dim),
            ]),
        }
    }

    pub fn title(&self) -> &'static str {
        match self.kind {
            DebugHitlKind::Reproduction => "Reproduce the bug",
            DebugHitlKind::Verification => "Verify the fix",
        }
    }
}

impl Drop for DebugHitlViewState {
    fn drop(&mut self) {
        // If the view is dropped without an explicit answer, cancel.
        if self.response_tx.is_some() {
            self.send_stale_cancel();
        }
    }
}
