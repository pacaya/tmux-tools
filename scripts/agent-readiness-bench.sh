#!/usr/bin/env bash
#
# agent-readiness-bench.sh — real-agent reliability harness for tmux-tools readiness
# detection. For each agent it spawns a read-only pane, waits for launch readiness,
# then fires a deterministic prompt N times and records how `prompt` completed
# (reason), how long detection took, and whether the expected answer token actually
# showed up. For claude it also runs a stale-`✓ done` race probe.
#
# This is NOT part of `cargo test` — it drives real agent binaries and tmux. Run it
# manually during development and report the summary.
#
# Usage:
#   scripts/agent-readiness-bench.sh [-n RUNS] [-a "claude codex cursor"] [-t THRESHOLD]
#
#   -n RUNS       prompts per agent (default 5)
#   -a AGENTS     space-separated agent list (default "claude codex cursor")
#   -t THRESHOLD  minimum success rate (0..1) before a non-zero exit (default 0.8)
#
# Requires: tmux, jq, a release/debug `tmux-tools` on PATH or built in ./target,
# and each agent's binary on PATH (claude / codex / cursor-agent).

set -u

RUNS=5
AGENTS="claude codex cursor"
THRESHOLD="0.8"
EXPECT_TOKEN="PONG"
PROMPT="Reply with exactly: ${EXPECT_TOKEN}"
# A claude completion must hold this long; the stale-done probe asserts the second
# prompt does not report completion faster than this.
READY_STABLE="2.0"

while getopts "n:a:t:h" opt; do
    case "$opt" in
        n) RUNS="$OPTARG" ;;
        a) AGENTS="$OPTARG" ;;
        t) THRESHOLD="$OPTARG" ;;
        h)
            grep '^#' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *) exit 2 ;;
    esac
done

# Resolve the tmux-tools binary: prefer PATH, then a local build.
TT=""
if command -v tmux-tools >/dev/null 2>&1; then
    TT="$(command -v tmux-tools)"
elif [ -x "./target/release/tmux-tools" ]; then
    TT="./target/release/tmux-tools"
elif [ -x "./target/debug/tmux-tools" ]; then
    TT="./target/debug/tmux-tools"
else
    echo "error: tmux-tools not found on PATH or in ./target (run: cargo build)" >&2
    exit 2
fi

for dep in tmux jq; do
    command -v "$dep" >/dev/null 2>&1 || { echo "error: $dep is required" >&2; exit 2; }
done

# Agent -> binary name on PATH.
agent_binary() {
    case "$1" in
        claude) echo "claude" ;;
        codex) echo "codex" ;;
        cursor) echo "cursor-agent" ;;
        *) echo "$1" ;;
    esac
}

overall_fail=0

for agent in $AGENTS; do
    bin="$(agent_binary "$agent")"
    if ! command -v "$bin" >/dev/null 2>&1; then
        echo "skip: ${agent} (binary '${bin}' not on PATH)"
        echo
        continue
    fi

    name="bench-${agent}-$$"
    echo "=== ${agent} (${RUNS} runs) ==="

    if ! "$TT" spawn-agent "$agent" --access read-only --name "$name" >/dev/null 2>&1; then
        echo "  FAIL: spawn-agent did not succeed"
        overall_fail=1
        echo
        continue
    fi

    # Wait for the agent to finish booting before the first prompt.
    "$TT" wait-idle --target "$name" --ready-stable-seconds "$READY_STABLE" >/dev/null 2>&1

    successes=0
    false_completions=0
    timeouts=0
    declare -a latencies=()

    for i in $(seq 1 "$RUNS"); do
        json="$("$TT" prompt --target "$name" "$PROMPT" \
            --ready-stable-seconds "$READY_STABLE" --format json 2>/dev/null)"
        reason="$(printf '%s' "$json" | jq -r '.reason // "error"')"
        dur="$(printf '%s' "$json" | jq -r '.duration_ms // 0')"
        out="$(printf '%s' "$json" | jq -r '.output_since_prompt // ""')"

        latencies+=("$dur")
        [ "$reason" = "timed_out" ] && timeouts=$((timeouts + 1))

        if printf '%s' "$out" | grep -q "$EXPECT_TOKEN"; then
            successes=$((successes + 1))
            verdict="ok"
        else
            verdict="MISS"
        fi
        printf '  run %d: reason=%-13s %5sms  %s\n' "$i" "$reason" "$dur" "$verdict"
    done

    # Stale-done race probe (claude only): fire a prompt immediately after one
    # completed; with the debounce a leftover `✓ done` from the prior turn must NOT
    # report completion in less than the stable window.
    race_note=""
    if [ "$agent" = "claude" ]; then
        race_ms="$("$TT" prompt --target "$name" "$PROMPT" \
            --ready-stable-seconds "$READY_STABLE" --format json 2>/dev/null \
            | jq -r '.duration_ms // 0')"
        # Allow a small scheduling margin below the stable window (in ms).
        floor_ms="$(awk -v s="$READY_STABLE" 'BEGIN { printf "%d", (s * 1000) - 200 }')"
        if [ "${race_ms:-0}" -lt "$floor_ms" ]; then
            race_note="STALE-DONE RACE: completed in ${race_ms}ms (< ${floor_ms}ms floor)"
            overall_fail=1
        else
            race_note="stale-done race ok (${race_ms}ms >= ${floor_ms}ms floor)"
        fi
    fi

    "$TT" kill --target "$name" >/dev/null 2>&1

    # Summary: success rate, mean + p95 latency.
    rate="$(awk -v s="$successes" -v n="$RUNS" 'BEGIN { printf "%.2f", (n > 0) ? s / n : 0 }')"
    mean="$(printf '%s\n' "${latencies[@]}" | awk '{ sum += $1; c++ } END { printf "%.0f", (c > 0) ? sum / c : 0 }')"
    p95="$(printf '%s\n' "${latencies[@]}" | sort -n | awk '{ a[NR] = $1 } END { if (NR == 0) { print 0 } else { i = int(0.95 * NR); if (i < 1) i = 1; print a[i] } }')"

    echo "  ---"
    echo "  success rate: ${rate} (${successes}/${RUNS})  mean: ${mean}ms  p95: ${p95}ms  timeouts: ${timeouts}  false-completions: ${false_completions}"
    [ -n "$race_note" ] && echo "  ${race_note}"

    if awk -v r="$rate" -v t="$THRESHOLD" 'BEGIN { exit !(r < t) }'; then
        echo "  FAIL: success rate ${rate} below threshold ${THRESHOLD}"
        overall_fail=1
    fi
    echo

    unset latencies
done

if [ "$overall_fail" -ne 0 ]; then
    echo "RESULT: FAIL"
    exit 1
fi
echo "RESULT: PASS"
