#!/usr/bin/env python3
"""A test shim around the real `orca` CLI for the live Orca contract run (P10.3).

It delegates every call to the real `orca` named by TT_SHIM_REAL and changes only
what a live Orca does not produce on demand:

- TT_SHIM_LOG: append each call's argv as one JSON line.
- TT_SHIM_CREATED: append {"handle": h} for every pane handle the real `terminal
  create` or `terminal split` returns, also when the shim then answers an error; the
  run's evidence that the pane is its own.
- TT_SHIM_SPLIT_TIMEOUT_STATE: the first `terminal split` without `--retry-split`
  runs the real split, keeps its answer in this file, then answers Orca's
  split-outcome error (`runtime_error`, data `splitId`, `tabId`,
  `splitMayStillComplete: true`). A `--retry-split` with that splitId answers the
  kept real answer; the real Orca has no record of the shim's splitId.
- TT_SHIM_LIST_TRUNCATED=1 / TT_SHIM_LIST_OMIT=1: `terminal list` reports
  `truncated: true` / a non-empty `hostScope.omittedHostIds`.
- TT_SHIM_STATUS_DROP: `status` reports its capabilities without this one.
- TT_SHIM_PS_ROW: a JSON file {"handle": h, "row": {...}}; `worktree ps` reports that
  agent row for h's pane (its paneKey is filled in), replacing any real row.
- TT_SHIM_SEND_RECEIPT: a JSON file holding a whole answer; `terminal send --enter`
  prints it instead of sending.
"""
import json
import os
import subprocess
import sys

REAL = os.environ["TT_SHIM_REAL"]
args = sys.argv[1:]
path = [a for a in args if not a.startswith("--")][:2]


def log():
    target = os.environ.get("TT_SHIM_LOG")
    if target:
        with open(target, "a") as handle:
            handle.write(json.dumps(args) + "\n")


def real(argv):
    done = subprocess.run([REAL] + argv, capture_output=True, text=True)
    if [a for a in argv if not a.startswith("--")][:2] in (["terminal", "create"], ["terminal", "split"]):
        created(done.stdout)
    return done.returncode, done.stdout, done.stderr


def created(out):
    target = os.environ.get("TT_SHIM_CREATED")
    if not target:
        return
    try:
        result = json.loads(out).get("result") or {}
    except ValueError:
        return
    for key in ("terminal", "split"):
        handle = (result.get(key) or {}).get("handle")
        if handle:
            with open(target, "a") as sink:
                sink.write(json.dumps({"handle": handle}) + "\n")


def answer(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.exit(0 if obj.get("ok") else 1)


def flag(name):
    for index, arg in enumerate(args):
        if arg == name and index + 1 < len(args):
            return args[index + 1]
        if arg.startswith(name + "="):
            return arg[len(name) + 1:]
    return None


log()

SHIM_SPLIT_ID = "shim-split-1"

if path == ["terminal", "split"] and "--retry-split" not in args:
    state = os.environ.get("TT_SHIM_SPLIT_TIMEOUT_STATE")
    if state and not os.path.exists(state):
        code, out, err = real(args)
        open(state, "w").write(out)
        split = json.loads(out).get("result", {}).get("split", {})
        tab = None
        if split.get("handle"):
            _, shown, _ = real(["terminal", "show", "--terminal", split["handle"], "--json"])
            tab = json.loads(shown)["result"]["terminal"]["tabId"]
        answer({"ok": False, "error": {
            "code": "runtime_error",
            "message": "Timed out waiting for split pane handle: pane %s in tab %s may "
                       "still open and run its command. (shim)" % (SHIM_SPLIT_ID, tab),
            "data": {"splitId": SHIM_SPLIT_ID, "tabId": tab,
                     "splitMayStillComplete": True}}})

if path == ["terminal", "split"] and flag("--retry-split") == SHIM_SPLIT_ID:
    state = os.environ.get("TT_SHIM_SPLIT_TIMEOUT_STATE")
    if state and os.path.exists(state):
        sys.stdout.write(open(state).read())
        sys.exit(0)

if path == ["terminal", "send"] and "--enter" in args and os.environ.get("TT_SHIM_SEND_RECEIPT"):
    answer(json.load(open(os.environ["TT_SHIM_SEND_RECEIPT"])))

code, out, err = real(args)
if code != 0 and not out.strip():
    sys.stderr.write(err)
    sys.exit(code)
try:
    parsed = json.loads(out)
except ValueError:
    sys.stdout.write(out)
    sys.stderr.write(err)
    sys.exit(code)
result = parsed.get("result") or {}

if path == ["terminal", "list"]:
    if os.environ.get("TT_SHIM_LIST_TRUNCATED") == "1":
        result["truncated"] = True
    if os.environ.get("TT_SHIM_LIST_OMIT") == "1":
        result.setdefault("hostScope", {})["omittedHostIds"] = ["shim-omitted-host"]

if path == ["status"] and os.environ.get("TT_SHIM_STATUS_DROP"):
    drop = os.environ["TT_SHIM_STATUS_DROP"]
    runtime = result.get("runtime", {})
    runtime["capabilities"] = [c for c in runtime.get("capabilities", []) if c != drop]

if path == ["worktree", "ps"] and os.environ.get("TT_SHIM_PS_ROW"):
    spec = json.load(open(os.environ["TT_SHIM_PS_ROW"]))
    _, shown, _ = real(["terminal", "show", "--terminal", spec["handle"], "--json"])
    terminal = json.loads(shown)["result"]["terminal"]
    key = terminal["tabId"] + ":" + terminal["leafId"]
    row = dict(spec["row"], paneKey=key)
    worktrees = result.get("worktrees", [])
    for worktree in worktrees:
        worktree["agents"] = [a for a in worktree.get("agents") or [] if a.get("paneKey") != key]
    home = next((w for w in worktrees if w.get("worktreeId") == terminal.get("worktreeId")), None)
    if home is None:
        home = {"worktreeId": terminal.get("worktreeId"), "agents": []}
        worktrees.append(home)
        result["worktrees"] = worktrees
    home["agents"].append(row)

parsed["result"] = result
sys.stdout.write(json.dumps(parsed) + "\n")
sys.exit(code)
