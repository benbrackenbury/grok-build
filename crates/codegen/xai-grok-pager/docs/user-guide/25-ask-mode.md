# Ask Mode

Ask mode is a Cursor-style read-only Q&A mode: the agent explores the codebase to answer questions but cannot edit files. Use it when you want explanations, navigation help, or design discussion without risking accidental changes.

---

## What Ask Mode Does

When ask mode is active, the agent:

1. Reads and searches the codebase to answer your questions
2. May use `ask_user_question` to clarify
3. Explains approaches and points to relevant files/symbols

Ask mode rejects **all** file edits — there is no plan-file carve-out like Plan mode. This holds in every permission mode, including always-approve. Shell/MCP tools that mutate the system are out of scope; stick to inspection.

If you ask the agent to implement a change, it should explain the approach and suggest switching to Agent (or Plan) mode instead of attempting the edit.

---

## How to Enter Ask Mode

- **Shift+Tab** — Cycle modes: Normal → **Ask** → Plan → Debug → Auto → Always-approve → Normal.
- **`/ask`** — Enter ask mode for the next prompts.
- **`/ask <question>`** — Enter ask mode and start a turn with that question.
- **`/ask off`** — Leave ask mode.

While active, the prompt shows an **`ask`** status flag.

---

## Tips

- Prefer concrete questions (“Where is session mode stored?”) over vague ones.
- Pair with Plan mode when you are ready to design an implementation without coding yet.
- Switch back to Normal (Agent) when you want the agent to make changes.
