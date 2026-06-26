#!/usr/bin/env bash
# Records Claude Code's agent state so tmux-tools can detect readiness from the
# status-line segment. See tmux-tools README, "Detecting readiness from a custom
# Claude status line". Wired from Claude Code hooks; reads the hook JSON payload
# on stdin. The mode is $1 (which hook fired).
#
# It maintains two per-session files under ${TMPDIR:-/tmp}/claude-state/:
#
#   <session_id>      one word — the main-agent state:
#                       working | waiting | permission | done
#   <session_id>.bg   TSV "<total>\t<sub>\t<mate>\t<shell>" — in-flight
#                     background tasks (subagents / teammates / background shells)
#
# The companion statusline-command.sh renders these as the leading
# "<indicator> | 🤖 …" segment of the status line, and tmux-tools' claude profile
# keys readiness off it: ready_regex = "(✓ done|⏸ waiting) \| 🤖", ready_lines = 0.
#
# This is the readiness-only core. The original it is adapted from also fires
# desktop/tmux notifications and debounces a deferred "done" ping when delegated
# background work finishes after the master stops; that is orthogonal to tmux-tools
# readiness and is intentionally omitted here. The state-file contract above is all
# tmux-tools needs — wire your own notifications separately if you want them.
#
# Requires: jq.

set -u

mode="${1:-}"
payload=$(cat 2>/dev/null || true)
sid=$(printf '%s' "$payload" | jq -r '.session_id // empty' 2>/dev/null)
[ -z "$sid" ] && sid="default"

dir="${TMPDIR:-/tmp}/claude-state"
mkdir -p "$dir" 2>/dev/null || true
main="$dir/$sid"
bg="$dir/$sid.bg"

# Emit "<total>\t<sub>\t<mate>\t<shell>" for background_tasks, excluding $1 (self).
# background_tasks is Claude Code's own authoritative list of in-flight async work.
bg_counts() {
  printf '%s' "$payload" | jq -r --arg self "$1" '
    (.background_tasks // [])
    | map(select(.id != $self))
    | "\(length)\t\(map(select(.type=="subagent"))|length)\t\(map(select(.type=="teammate"))|length)\t\(map(select(.type=="shell"))|length)"
  ' 2>/dev/null
}

case "$mode" in
  cleanup)
    # SessionEnd: drop the state files.
    rm -f "$main" "$bg" 2>/dev/null
    ;;
  working)
    # UserPromptSubmit: a new turn started.
    printf 'working' > "$main" 2>/dev/null
    ;;
  waiting)
    # Notification(idle_prompt): idle, awaiting input.
    printf 'waiting' > "$main" 2>/dev/null
    ;;
  permission)
    # Notification(permission_prompt): awaiting an approval prompt. But
    # AskUserQuestion / plan-approval also emit a permission_prompt, so keep an
    # already-set "waiting" rather than flip it to "permission".
    [ "$(cat "$main" 2>/dev/null)" = "waiting" ] && exit 0
    printf 'permission' > "$main" 2>/dev/null
    ;;
  pretool)
    # PreToolUse: subagent tool calls (payload carries agent_id) must NOT touch
    # the master state. The interactive tools mean Claude is waiting on you.
    aid=$(printf '%s' "$payload" | jq -r '.agent_id // empty' 2>/dev/null)
    [ -n "$aid" ] && exit 0
    tool=$(printf '%s' "$payload" | jq -r '.tool_name // empty' 2>/dev/null)
    case "$tool" in
      AskUserQuestion|ExitPlanMode) printf 'waiting' > "$main" 2>/dev/null ;;
      *)                            printf 'working' > "$main" 2>/dev/null ;;
    esac
    ;;
  bg)
    # SubagentStop: recount in-flight background work, excluding the subagent
    # whose own SubagentStop this is (it is finishing now).
    self=$(printf '%s' "$payload" | jq -r '.agent_id // empty' 2>/dev/null)
    counts=$(bg_counts "$self")
    [ -z "$counts" ] && counts=$(printf '0\t0\t0\t0')
    printf '%s' "$counts" > "$bg" 2>/dev/null
    ;;
  done)
    # Stop / SessionStart: the master turn is complete. Record the current
    # background count too, so a `✓ done` master with subagents still running
    # renders as `⚙ N bg` (busy) rather than a premature `✓ done`.
    self=$(printf '%s' "$payload" | jq -r '.agent_id // empty' 2>/dev/null)
    counts=$(bg_counts "$self")
    [ -z "$counts" ] && counts=$(printf '0\t0\t0\t0')
    printf '%s' "$counts" > "$bg" 2>/dev/null
    printf 'done' > "$main" 2>/dev/null
    ;;
esac

exit 0
