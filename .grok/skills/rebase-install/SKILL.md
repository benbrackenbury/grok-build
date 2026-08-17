---
name: rebase-install
description: >
  Rebase this fork onto latest upstream/main, resolve replay conflicts, push
  the rewritten main, then build xai-grok-pager-bin and install it as
  ~/.local/bin/spacex. Use when the user asks to rebase upstream, sync
  upstream/main, ship a local grok-build binary, copy to their bin dir, or
  runs /rebase-install.
metadata:
  short-description: "Rebase onto upstream/main, push, install spacex"
  user-invocable: true
---

# /rebase-install

Replay this fork's feature commits onto `upstream/main`, push `origin/main`,
build the pager, and install it as `spacex`.

## Remotes and install target

| Name | Role |
|---|---|
| `upstream` | `xai-org/grok-build` — source of truth for synced monorepo commits |
| `origin` | this fork — rewritten `main` is force-pushed here after rebase |
| install | `cp target/debug/xai-grok-pager ~/.local/bin/spacex` |

Do not push to `upstream`. After a rebase, `git push --force-with-lease origin main`.

## Feature commits to keep

This fork's identity lives in commits *above* the last `Synced from monorepo`
(or equivalent upstream tip). Typical stack, oldest first:

1. `SessionMode::Debug` + debug notification types
2. enter/exit/await/read debug-mode tools
3. inject debug tools into the agent toolset
4. `DebugModeTracker` + session mode wiring
5. park debug await tools for Proceed / Verify
6. `/debug-mode` + Shift+Tab debug cycle
7. Proceed / Mark Fixed debug HITL chrome
8. debug-mode user-guide docs
9. SpaceXAI welcome mark
10. system theme / transparent backgrounds
11. Cursor-style ask mode

Preserve that behavior. Drop or squash only empty replays (`git rebase --skip`
when a pick is already in upstream).

## Steps

1. **Dirty tree.** Stash tracked changes (`git stash push -u -m rebase-install`)
   so rebase can start. Do not commit unrelated WIP into the rebase.
2. **Fetch.** `git fetch upstream` (and `git fetch origin` if you will
   `--force-with-lease` against it).
3. **Rebase.** From `main`: `git rebase upstream/main`.
4. **Conflicts.** Resolve per the policy below, then
   `git add` + `git rebase --continue`. Never `--skip` a non-empty feature
   commit. Abort only if the tree is irrecoverable; say so and stop.
5. **Push.** `git push --force-with-lease origin main`.
6. **Restore stash.** `git stash pop` if step 1 stashed. Resolve any pop
   conflicts the same way as rebase conflicts.
7. **Build.** From the repo root:
   `cargo build -p xai-grok-pager-bin`
   (debug, matching `~/.local/bin/spacex` history). Use `--release` only if
   the user asked. Full-workspace builds are too slow — always `-p`.
8. **Install.** `cp target/debug/xai-grok-pager ~/.local/bin/spacex`
   (`target/release/xai-grok-pager` when this run was `--release`).
   Then ad-hoc resign so macOS AMFI does not SIGKILL the copied binary:
   `xattr -c ~/.local/bin/spacex; codesign --force --sign - ~/.local/bin/spacex`.
   A bare `cp` of a linker-signed debug Mach-O is `valid on disk` to
   `codesign -v` but taskgated still kills it (`Code Signature Invalid`).
   Confirm `spacex --version` and print `ls -lh ~/.local/bin/spacex`.

## Conflict policy

Prefer the *new API / structure from upstream* and *re-apply fork behavior*
on top. Do not take ours or theirs blindly.

| Kind | Take |
|---|---|
| `SOURCE_REV`, generated root `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` | upstream, then `cargo generate-lockfile` / rebuild if the lockfile is inconsistent |
| Renames and field moves (`persistence_tx` → `persistence.tx`, new notification variants, tool-registry shape) | upstream names; update fork call sites to compile |
| Debug mode, ask mode, Plan/Ask/Debug Shift+Tab cycle, HITL chrome | fork behavior |
| SpaceXAI welcome logos / branding, system/transparent theme | fork behavior |
| User-guide docs that describe fork features | keep the fork docs; add upstream sections if they are new |
| Tests that fail because a fork feature changed a default | update the test to the fork's intended behavior |

When the same hunk mixes an upstream rename with a fork feature, write the
merged form (new names, fork logic). After each continued commit, if the
crate would not compile, fix it in that commit rather than leaving a broken
mid-rebase tree.

## Stop conditions

- `upstream` remote missing: add it, do not invent another source.
- Rebase would drop a listed feature commit: stop and report.
- `--force-with-lease` rejected: stop; origin moved. Do not `--force`.
- Build fails after a clean rebase: fix compile errors from the replay, do
  not revert the rebase.
