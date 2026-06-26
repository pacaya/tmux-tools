#!/usr/bin/env bash
# Claude Code status line that emits a Codex-style state indicator as the LEADING
# segment, so tmux-tools can detect readiness from it. Renders one line:
#
#   <indicator> | 🤖 <model> | ⎇ <branch> <status> | 📁 <dir> | [bar] Nk/Mk (P%)
#
# where <indicator> is one of (priority high→low):
#   🔐 permission   awaiting an approval prompt        (busy)
#   ⚡ working      generating                         (busy)
#   ⚙ N bg (…)     N background subagents/teammates    (busy)
#   ⏸ waiting       idle, awaiting input               (READY)
#   ✓ done          turn complete                      (READY)
#
# The indicator is driven by the companion statusline-state.sh hook, which writes
# the per-session state files this reads — Claude does NOT pass the state to the
# status line directly. tmux-tools' claude profile keys readiness off the two
# READY states: ready_regex = "(✓ done|⏸ waiting) \| 🤖", ready_lines = 0. The
# unique " | 🤖" anchor keeps that match off conversation text; everything after
# the indicator is cosmetic (model / git / context) and irrelevant to readiness.
#
# Requires: jq, git. Wire via settings.json: "statusLine".command.

input=$(cat)

# --- Leading state indicator (the readiness-critical part) -------------------
session_id=$(printf '%s' "$input" | jq -r '.session_id // empty')
sdir="${TMPDIR:-/tmp}/claude-state"
state=$(cat "$sdir/$session_id" 2>/dev/null)

BG_N=0; BG_SUB=0; BG_MATE=0; BG_SH=0
[ -f "$sdir/$session_id.bg" ] && IFS=$'\t' read -r BG_N BG_SUB BG_MATE BG_SH < "$sdir/$session_id.bg"
BG_N=${BG_N:-0}
bg_detail=""
if [ "$BG_N" -gt 0 ] 2>/dev/null; then
  [ "${BG_SUB:-0}"  -gt 0 ] 2>/dev/null && bg_detail="${bg_detail}${bg_detail:+, }${BG_SUB} sub"
  [ "${BG_MATE:-0}" -gt 0 ] 2>/dev/null && bg_detail="${bg_detail}${bg_detail:+, }${BG_MATE} mate"
  [ "${BG_SH:-0}"   -gt 0 ] 2>/dev/null && bg_detail="${bg_detail}${bg_detail:+, }${BG_SH} sh"
  [ -n "$bg_detail" ] && bg_detail=" (${bg_detail})"
fi

# Priority: permission > working > background work > waiting > done.
case "$state" in
  permission) seg="🔐 permission | " ;;
  working)    seg="⚡ working | " ;;
  *)
    if   [ "$BG_N" -gt 0 ] 2>/dev/null; then seg="⚙ ${BG_N} bg${bg_detail} | "
    elif [ "$state" = "waiting" ];      then seg="⏸ waiting | "
    elif [ "$state" = "done" ];         then seg="✓ done | "
    else                                     seg=""
    fi
    ;;
esac

# --- Cosmetic tail: model | git | dir | context % ----------------------------
model=$(printf '%s' "$input" | jq -r '.model.display_name // .model.id')
cwd=$(printf '%s' "$input" | jq -r '.workspace.current_dir // .cwd')
cd "$cwd" 2>/dev/null || cd /

if git rev-parse --git-dir >/dev/null 2>&1; then
  branch=$(git symbolic-ref --short HEAD 2>/dev/null || git rev-parse --short HEAD 2>/dev/null)
  if git diff-index --quiet HEAD -- 2>/dev/null && [ -z "$(git ls-files --others --exclude-standard)" ]; then
    git_seg="⎇ ${branch} clean"
  else
    n=$(( $(git status --porcelain 2>/dev/null | wc -l | tr -d ' ') ))
    git_seg="⎇ ${branch} dirty (${n})"
  fi
else
  git_seg="no git"
fi

size=$(printf '%s' "$input" | jq -r '.context_window.context_window_size // 0')
used=$(printf '%s' "$input" | jq -r '
  (.context_window.current_usage // {})
  | (.input_tokens // 0) + (.output_tokens // 0)
    + (.cache_creation_input_tokens // 0) + (.cache_read_input_tokens // 0)')
pct=0
[ "${size:-0}" -gt 0 ] 2>/dev/null && pct=$(( used * 100 / size ))
ctx="$(( used / 1000 ))k/$(( size / 1000 ))k (${pct}%)"

printf '%s🤖 %s | %s | 📁 %s | %s' "$seg" "$model" "$git_seg" "$(basename "$cwd")" "$ctx"
