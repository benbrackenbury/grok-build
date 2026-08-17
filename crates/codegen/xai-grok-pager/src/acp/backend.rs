//! ACP backend selection: in-process Grok shell vs Cursor Agent (subscription).
//!
//! Cursor reuses the machine's existing `cursor-agent` login. This crate never
//! implements a Cursor sign-in flow.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

use anyhow::{Result, anyhow};

/// Which ACP agent this pager process talks to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum AgentBackend {
    /// In-process Grok / SpaceXAI shell (the default).
    #[default]
    Grok,
    /// `cursor-agent acp`, authenticated with the local Cursor CLI login.
    Cursor,
}

impl AgentBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grok => "grok",
            Self::Cursor => "cursor",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Grok => "Grok Build",
            Self::Cursor => "Cursor",
        }
    }

    pub fn parse_name(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "grok" | "spacex" | "spacexai" | "xai" => Some(Self::Grok),
            "cursor" => Some(Self::Cursor),
            _ => None,
        }
    }

    pub fn is_cursor(self) -> bool {
        matches!(self, Self::Cursor)
    }
}

/// CLI / env / config key. Not a public user-facing env name beyond this crate.
pub const GROK_ACP_BACKEND_ENV: &str = "GROK_ACP_BACKEND";

/// Set on backend-switch relaunch so the next process can toast the change.
pub const GROK_ACP_BACKEND_SWITCH_ENV: &str = "GROK_ACP_BACKEND_SWITCH";

/// Override the `cursor-agent` binary. When unset, PATH and `~/.local/bin` are searched.
pub const CURSOR_AGENT_ENV: &str = "CURSOR_AGENT";

static CURRENT: AtomicU8 = AtomicU8::new(0);
static RELAUNCH: Mutex<Option<AgentBackend>> = Mutex::new(None);

fn encode(backend: AgentBackend) -> u8 {
    match backend {
        AgentBackend::Grok => 0,
        AgentBackend::Cursor => 1,
    }
}

fn decode(v: u8) -> AgentBackend {
    match v {
        1 => AgentBackend::Cursor,
        _ => AgentBackend::Grok,
    }
}

pub fn set_current(backend: AgentBackend) {
    CURRENT.store(encode(backend), Ordering::Release);
}

pub fn current() -> AgentBackend {
    decode(CURRENT.load(Ordering::Acquire))
}

pub fn set_relaunch(backend: AgentBackend) {
    if let Ok(mut slot) = RELAUNCH.lock() {
        *slot = Some(backend);
    }
}

pub fn take_relaunch() -> Option<AgentBackend> {
    RELAUNCH.lock().ok().and_then(|mut slot| slot.take())
}

/// CLI > `GROK_ACP_BACKEND` > `[backend].provider` > Grok.
pub fn resolve(cli: Option<AgentBackend>, raw_config: &toml::Value) -> AgentBackend {
    if let Some(cli) = cli {
        return cli;
    }
    if let Ok(raw) = std::env::var(GROK_ACP_BACKEND_ENV)
        && let Some(parsed) = AgentBackend::parse_name(&raw)
    {
        return parsed;
    }
    if let Some(parsed) = raw_config
        .get("backend")
        .and_then(|b| b.get("provider"))
        .and_then(|v| v.as_str())
        .and_then(AgentBackend::parse_name)
    {
        return parsed;
    }
    AgentBackend::Grok
}

/// Resolve against the on-disk effective config (CLI still wins).
pub fn resolve_from_disk(cli: Option<AgentBackend>) -> AgentBackend {
    let cfg = xai_grok_shell::config::load_effective_config()
        .unwrap_or(toml::Value::Table(Default::default()));
    resolve(cli, &cfg)
}

pub fn persist(backend: AgentBackend) -> std::io::Result<()> {
    let path = xai_grok_shell::util::grok_home::grok_home().join("config.toml");
    persist_at(&path, backend)
}

pub(crate) fn persist_at(path: &Path, backend: AgentBackend) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let Some(mut doc) = crate::config_toml_edit::read_config_document_for_edit(path) else {
        return Ok(());
    };
    doc["backend"]["provider"] = toml_edit::value(backend.as_str());
    std::fs::write(path, doc.to_string())
}

/// Consume the one-shot switch hint set by a backend relaunch.
pub fn take_switch_hint() -> Option<AgentBackend> {
    let raw = std::env::var_os(GROK_ACP_BACKEND_SWITCH_ENV);
    if raw.is_some() {
        // SAFETY: startup only, before the event loop and before children inherit env.
        unsafe { std::env::remove_var(GROK_ACP_BACKEND_SWITCH_ENV) };
    }
    raw.as_deref()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(AgentBackend::parse_name)
}

pub fn find_cursor_agent() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os(CURSOR_AGENT_ENV) {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    if let Some(found) = find_on_path("cursor-agent") {
        return Some(found);
    }
    let home_local = dirs_next_home()
        .map(|h| h.join(".local/bin/cursor-agent"))
        .filter(|p| p.is_file());
    home_local
}

fn dirs_next_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
        #[cfg(windows)]
        {
            let exe = dir.join(format!("{name}.exe"));
            if exe.is_file() {
                return Some(exe);
            }
            let cmd = dir.join(format!("{name}.cmd"));
            if cmd.is_file() {
                return Some(cmd);
            }
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorLogin {
    pub email: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorProbe {
    Ready(CursorLogin),
    NotLoggedIn,
    Missing,
}

/// Locate `cursor-agent` and confirm the existing machine login.
pub fn probe_cursor() -> CursorProbe {
    let Some(bin) = find_cursor_agent() else {
        return CursorProbe::Missing;
    };
    match cursor_status(&bin) {
        Ok(true) => CursorProbe::Ready(CursorLogin {
            email: cursor_status_email(&bin),
        }),
        Ok(false) => CursorProbe::NotLoggedIn,
        Err(_) => CursorProbe::NotLoggedIn,
    }
}

pub fn cursor_not_ready_message(probe: &CursorProbe) -> String {
    match probe {
        CursorProbe::Ready(_) => String::new(),
        CursorProbe::Missing => {
            "cursor-agent not found. Install the Cursor CLI and sign in with `cursor-agent login`."
                .into()
        }
        CursorProbe::NotLoggedIn => {
            "Cursor Agent is installed but not signed in. Run `cursor-agent login` in a terminal, then try /cursor again.".into()
        }
    }
}

pub fn ensure_cursor_ready() -> Result<PathBuf> {
    match probe_cursor() {
        CursorProbe::Ready(_) => find_cursor_agent()
            .ok_or_else(|| anyhow!("cursor-agent disappeared after a successful probe")),
        other => Err(anyhow!("{}", cursor_not_ready_message(&other))),
    }
}

fn cursor_status(bin: &Path) -> Result<bool> {
    let output = Command::new(bin)
        .args(["status", "--format", "json"])
        .output()?;
    if !output.status.success() {
        return Ok(false);
    }
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    Ok(cursor_status_authenticated(&parsed))
}

fn cursor_status_email(bin: &Path) -> Option<String> {
    let output = Command::new(bin)
        .args(["status", "--format", "json"])
        .output()
        .ok()?;
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).ok()?;
    parsed
        .get("userInfo")
        .and_then(|u| u.get("email"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

pub(crate) fn cursor_status_authenticated(value: &serde_json::Value) -> bool {
    if value
        .get("isAuthenticated")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return true;
    }
    value
        .get("status")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("authenticated"))
}

/// Rebuild argv and exec into the chosen backend. Starts a fresh session —
/// Grok and Cursor transcripts are not interchangeable.
pub fn exec_backend_relaunch(backend: AgentBackend) -> std::io::Result<()> {
    let exe = std::env::current_exe()?;
    let args = build_backend_relaunch_args(std::env::args_os(), backend);
    let mut cmd = Command::new(&exe);
    cmd.args(&args);
    cmd.env(GROK_ACP_BACKEND_ENV, backend.as_str());
    cmd.env(GROK_ACP_BACKEND_SWITCH_ENV, backend.as_str());

    eprintln!(
        "Switching to {}… (switch back with /{})",
        backend.display_name(),
        match backend {
            AgentBackend::Grok => "cursor",
            AgentBackend::Cursor => "grok",
        }
    );
    let _ = std::io::Write::flush(&mut std::io::stdout());
    let _ = std::io::Write::flush(&mut std::io::stderr());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(std::io::Error::other(format!(
            "failed to exec backend relaunch: {err}"
        )))
    }

    #[cfg(windows)]
    {
        cmd.stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());
        unsafe {
            windows_sys::Win32::System::Console::SetConsoleCtrlHandler(None, 1);
        }
        std::thread::sleep(std::time::Duration::from_millis(150));
        #[allow(clippy::disallowed_methods)]
        let mut child = cmd.spawn()?;
        let status = child.wait()?;
        std::process::exit(status.code().unwrap_or(0));
    }

    #[cfg(not(any(unix, windows)))]
    {
        Err(std::io::Error::other(
            "backend relaunch unsupported on this platform",
        ))
    }
}

/// Drop session-selection / one-shot flags and force `--backend <name>`.
pub(crate) fn build_backend_relaunch_args(
    current_args: impl IntoIterator<Item = impl AsRef<std::ffi::OsStr>>,
    backend: AgentBackend,
) -> Vec<std::ffi::OsString> {
    use std::ffi::OsString;

    let mut iter = current_args
        .into_iter()
        .map(|a| a.as_ref().to_os_string())
        .peekable();
    let _ = iter.next();

    let mut out: Vec<OsString> = Vec::new();
    while let Some(arg) = iter.next() {
        let s = arg.to_string_lossy().into_owned();
        if s == "--" {
            break;
        }
        if matches!(
            s.as_ref(),
            "--backend"
                | "--cursor"
                | "--grok"
                | "--resume"
                | "-r"
                | "--continue"
                | "-c"
                | "--fork-session"
                | "--session-id"
                | "--load"
                | "--worktree"
                | "-w"
                | "--worktree-ref"
                | "--restore-code"
        ) {
            if s.contains('=') {
                continue;
            }
            let _ = iter.next();
            continue;
        }
        if s.starts_with("--backend=")
            || s.starts_with("--resume=")
            || s.starts_with("--session-id=")
            || s.starts_with("--load=")
            || s.starts_with("--worktree=")
            || s.starts_with("--worktree-ref=")
        {
            continue;
        }
        if !s.starts_with('-') {
            // Positional prompt — do not re-submit.
            break;
        }
        out.push(arg);
        // Value-taking flags we did not drop: keep the following token if it
        // does not look like another flag. `--backend` is already dropped.
        if looks_like_value_taking_flag(&s)
            && let Some(next) = iter.peek()
            && !next.to_string_lossy().starts_with('-')
        {
            out.push(iter.next().unwrap());
        }
    }
    out.push(OsString::from(match backend {
        AgentBackend::Cursor => "--cursor",
        AgentBackend::Grok => "--grok",
    }));
    out.push(OsString::from("--no-leader"));
    out
}

fn looks_like_value_taking_flag(flag: &str) -> bool {
    matches!(
        flag,
        "--model"
            | "-m"
            | "--cwd"
            | "--rules"
            | "--system-prompt-override"
            | "--system-prompt"
            | "--reasoning-effort"
            | "--effort"
            | "--permission-mode"
            | "--output-format"
            | "--leader-socket"
            | "--debug-file"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn parse_names() {
        assert_eq!(AgentBackend::parse_name("Cursor"), Some(AgentBackend::Cursor));
        assert_eq!(AgentBackend::parse_name("spacexai"), Some(AgentBackend::Grok));
        assert_eq!(AgentBackend::parse_name("nope"), None);
    }

    #[test]
    fn resolve_prefers_cli() {
        let cfg: toml::Value = toml::from_str("[backend]\nprovider = \"cursor\"\n").unwrap();
        assert_eq!(
            resolve(Some(AgentBackend::Grok), &cfg),
            AgentBackend::Grok
        );
    }

    #[test]
    fn resolve_reads_config() {
        let cfg: toml::Value = toml::from_str("[backend]\nprovider = \"cursor\"\n").unwrap();
        // Isolate from a leftover process env.
        let prev = std::env::var_os(GROK_ACP_BACKEND_ENV);
        unsafe { std::env::remove_var(GROK_ACP_BACKEND_ENV) };
        let got = resolve(None, &cfg);
        if let Some(v) = prev {
            unsafe { std::env::set_var(GROK_ACP_BACKEND_ENV, v) };
        }
        assert_eq!(got, AgentBackend::Cursor);
    }

    #[test]
    fn persist_writes_provider() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, "[ui]\nvim_mode = false\n").unwrap();
        persist_at(&path, AgentBackend::Cursor).unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("provider") && body.contains("cursor") && body.contains("vim_mode"));
    }

    #[test]
    fn status_json_authenticated() {
        let v = serde_json::json!({
            "status": "authenticated",
            "isAuthenticated": true,
            "userInfo": { "email": "a@b.c" }
        });
        assert!(cursor_status_authenticated(&v));
        let v = serde_json::json!({ "status": "logged_out", "isAuthenticated": false });
        assert!(!cursor_status_authenticated(&v));
    }

    #[test]
    fn relaunch_args_force_backend_and_drop_resume() {
        let args = build_backend_relaunch_args(
            [
                "spacex",
                "--resume",
                "sess-1",
                "--model",
                "grok-4.6",
                "do the thing",
            ],
            AgentBackend::Cursor,
        );
        let rendered: Vec<String> = args
            .iter()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert!(!rendered.iter().any(|s| s == "sess-1" || s == "--resume"));
        assert!(rendered.contains(&"--cursor".into()));
        assert!(!rendered.iter().any(|s| s == "--backend"));
        assert!(rendered.contains(&"--model".into()));
        assert!(rendered.contains(&"grok-4.6".into()));
        assert!(!rendered.iter().any(|s| s == "do the thing"));
    }
}
