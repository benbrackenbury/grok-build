# Debug Mode

Debug mode is a structured debugging loop inspired by Cursor’s Debug Mode: the agent forms multiple hypotheses, instruments code with runtime logs, asks you to reproduce the bug, analyzes NDJSON evidence, applies a minimal fix, then asks you to verify before removing instrumentation.

Use it when you can reproduce a bug but the root cause is unclear from reading the code alone.

---

## How to enter

- **Shift+Tab** — Cycle modes: Normal → Ask → Plan → **Debug** → Auto → Always-approve → Normal.
- **`/debug-mode`** — Enter debug mode for the next prompts.
- **`/debug-mode <bug description>`** — Enter debug mode and start a turn with that description.
- **`/debug-mode off`** — Leave debug mode.
- Agent tool **`enter_debug_mode`** — Agent-initiated entry (requires approval like other tools).

> **Note:** `/debug` (without `-mode`) is a separate command for TUI diagnostic overlays (scroll/FPS/log). It does not enter this agent loop.

While active, the prompt shows a **`debug`** status flag. During human gates it shows **`debug: proceed`** or **`debug: verify`**.

### Human gates (keys)

| Gate | Keys |
|------|------|
| Reproduce | **`p`** / **Enter** = Proceed · **`q`** = Abandon · **Esc** = Cancel wait |
| Verify | **`f`** = Mark Fixed · **`b`** = Still broken · **`q`** = Abandon · **Esc** = Cancel |

---

## Loop

1. **Explore** — Agent reads the codebase and writes labeled hypotheses (A/B/C…) to the session scratch file (`debug.md` under the session directory).
2. **Instrument** — Agent adds tagged log regions (`// #region agent log` / `# region agent log`) that append NDJSON lines to the session `debug.log`.
3. **Reproduce** — Agent calls `await_debug_reproduction` with concrete steps. Follow the steps (restart/rebuild if needed).
4. **Analyze** — After you proceed, agent reads logs via `read_debug_logs`, confirms/rejects hypotheses, and applies a **small** fix while keeping instrumentation.
5. **Verify** — Agent calls `await_debug_verification`. Re-check the behavior.
6. **Cleanup** — On **Mark Fixed**, agent strips agent-log regions and calls `exit_debug_mode`.

---

## Log format

One JSON object per line (NDJSON), for example:

```json
{"id":"log_1","timestamp":1710000000000,"location":"src/app.ts:42","message":"before total","data":{"cartId":"x"},"sessionId":"debug-session","runId":"run1","hypothesisId":"A"}
```

Default log path: `~/.grok/sessions/<encoded-cwd>/<session-id>/debug.log`  
Scratch notes: same directory as `debug.md`.

HTTP ingest (like Cursor’s loopback NDJSON server) is **not** required for v1; instrumentation appends to the file.

---

## Tips

- Give expected vs actual behavior, stack traces, and exact repro steps.
- Prefer small, hypothesis-discriminating logs over flooding every line.
- Do not commit leftover `#region agent log` blocks; search the workspace if you abandon mid-session.
- Always-approve still works for edit tools, but human Proceed / Mark Fixed gates still apply when the agent parks those tools.
