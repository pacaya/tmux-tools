# Fast Claude readiness via a custom status line

An opt-in bundle that gives tmux-tools a **fast, precise** "is Claude done?" signal
instead of the default idle-quiescence fallback.

By default the built-in `claude` profile ships **no** `ready_regex` — Claude Code's
native footer can't tell idle from busy (the `← for agents` hint shows while
generating too), so `wait-idle` / `prompt` fall back to idle detection. That's
reliable but costs up to `--idle-seconds` (default 2s) per turn, because it waits
for the pane to go quiet.

This bundle replaces that with an explicit state indicator rendered as the **leading
segment** of Claude's status line, which tmux-tools matches directly:

```
✓ done | 🤖 Opus 4.8 (1M context) | ⎇ main clean | 📁 myrepo | [█▒░░░] 190k/1000k (19%)
└──────┘
 readiness indicator                ← ready_regex = "(✓ done|⏸ waiting) \| 🤖"
```

## How it works

Claude does **not** pass its working/idle state to the status line. So a hook
(`statusline-state.sh`) listens to Claude Code's lifecycle hooks and records the
state to two per-session files under `${TMPDIR:-/tmp}/claude-state/`, and the status
line (`statusline-command.sh`) reads them back and renders the indicator.

**State-file contract** (write these yourself if you'd rather wire your own hooks):

| File | Contents |
| --- | --- |
| `<session_id>` | one word: `working` \| `waiting` \| `permission` \| `done` |
| `<session_id>.bg` | TSV `<total>\t<subagents>\t<teammates>\t<shells>` of in-flight background tasks |

Indicator priority (high→low): `🔐 permission` › `⚡ working` › `⚙ N bg` › `⏸ waiting` › `✓ done`.
Only `✓ done` and `⏸ waiting` are **ready**; the rest are **busy**.

**Why `ready_lines = 0`.** Completed subagent rows persist *below* the status line
and push it up — by `N + 3` non-blank lines for `N` subagents — so any fixed scan
window is eventually exceeded by a tall enough footer. `0` scans every non-blank
line; the unique ` | 🤖` anchor keeps that safe from false-matching conversation
text.

**Why this is correct for background work.** While subagents/teammates run, the
hook keeps the indicator at `⚙ N bg` (busy → `ready_regex` won't fire) *and* Claude's
footer animates (per-second task timers → idle stays suppressed). Completion fires
only once the master has stopped **and** the background count returns to 0. The one
case neither signal catches is a *static* `🔐 permission` prompt in a non-bypass
permission mode — irrelevant when driving with `--access full-access`.

## Install

1. Copy the two scripts to `~/.claude/` and make them executable:
   ```sh
   cp statusline-command.sh statusline-state.sh ~/.claude/
   chmod +x ~/.claude/statusline-command.sh ~/.claude/statusline-state.sh
   ```
2. Merge the keys from `settings.hooks.json` into `~/.claude/settings.json`
   (the `statusLine` block and the eight `hooks` entries). If you already have a
   `statusLine` or some of these hooks, append rather than overwrite.
3. Merge the `[claude]` block from `agents.toml` into
   `~/.config/tmux-tools/agents.toml`.
4. Restart Claude Code (or start a new session) so the hooks/status line load.

Requires `jq` and `git` on `PATH`.

## Verify

```sh
tmux-tools spawn-agent claude --access full-access --name probe
tmux-tools capture --target probe --lines 3      # bottom line should start with "✓ done | 🤖"
tmux-tools prompt  --target probe "say hi" --format json   # reason should be "ready_matched"
tmux-tools kill    --target probe
```

If the indicator segment never appears, the hooks aren't firing — check the hook
commands' paths in `settings.json` and that `jq` is installed. With the segment
absent, readiness simply falls back to idle (still correct, just slower).

## Caveats

- **Version-sensitive chrome.** The glyphs and the ` | 🤖` anchor are a convention
  *this bundle* establishes, so they're stable as long as you control the scripts —
  but Claude Code's hook payloads (`background_tasks`, `agent_id`, notification
  matchers) can change between releases. Re-check after a major Claude upgrade.
  Validated against Claude Code 2.1.x.
- **Notifications omitted.** `statusline-state.sh` here is the readiness-only core.
  The setup it's adapted from also fires desktop/tmux notifications and debounces a
  deferred "done" ping when delegated work finishes after the master stops — that's
  orthogonal to tmux-tools and left out for clarity. The state-file contract above
  is all tmux-tools needs.

See the main [README](../../README.md) section "Detecting readiness from a custom
Claude status line" for the surrounding rationale.
