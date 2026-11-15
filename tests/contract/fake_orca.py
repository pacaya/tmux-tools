#!/usr/bin/env python3
"""A stateful stand-in for the `orca` CLI, used by the default suite's Orca regressions.

It never reaches a real Orca runtime. Its panes live in the JSON file FAKE_ORCA_STATE.

- FAKE_ORCA_LOG: append each call's argv as one JSON line.
- FAKE_ORCA_UNAVAILABLE=1: every command answers `runtime_unavailable`.
- FAKE_ORCA_ON_LIST_SPAWN=<name>: the first `terminal list` takes its answer, then
  acts as a concurrent spawn of <name> (opens a pane and writes its Pane record)
  before it prints that answer.
- FAKE_ORCA_LOCK_REGISTRY=1: `terminal create` makes the runtime's registry directory
  read-only, so the CLI cannot write the new handle; `terminal close` restores it
  (also when it then fails).
- FAKE_ORCA_CLOSE_FAIL=1: `terminal close` fails.
- FAKE_ORCA_FOREIGN_ON_CREATE=<title>: `terminal create` also opens an unrelated pane
  titled <title> in the same worktree, as another session would meanwhile.
- FAKE_ORCA_CREATE_LATE=<n>: `terminal create` answers a timeout error; its pane
  opens only on the <n>-th `terminal list` after it.
- FAKE_ORCA_SCROLLBACK=<n>: every pane holds the lines `line1`..`line<n>`; its
  screen is the last 5, `terminal read --limit <k>` answers the last k, and a read
  without either answers all.
- FAKE_ORCA_SPLIT_LATE=<n>: `terminal split` answers a timeout error without a
  leafId; its untitled pane opens in the target's tab only on the <n>-th
  `terminal list` after it.

- FAKE_ORCA_EMPTY_LISTS=<n>: the first <n> `terminal list` calls (counted in the
  state as `empty_lists`) exit 0 with empty stdout, as Orca 1.4.253 did for a few
  seconds after a pane was created.

A split pane opens in the tab of the pane it splits, without a title.
"""
import json
import os
import sys

RUNTIME = "fake-runtime"
WORKTREE = "fake-worktree"
args = sys.argv[1:]
path = [a for a in args if not a.startswith("--")][:2]


def flag(name):
    for index, arg in enumerate(args):
        if arg == name and index + 1 < len(args):
            return args[index + 1]
        if arg.startswith(name + "="):
            return arg[len(name) + 1:]
    return None


def answer(result):
    sys.stdout.write(json.dumps({"ok": True, "result": result}) + "\n")
    sys.exit(0)


def fail(code, message, data=None):
    sys.stdout.write(json.dumps({"ok": False, "error": {
        "code": code, "message": message, "data": data or {}}}) + "\n")
    sys.exit(1)


def load():
    try:
        return json.load(open(os.environ["FAKE_ORCA_STATE"]))
    except (OSError, ValueError):
        return {"next": 1, "terminals": [], "flags": []}


def save(state):
    json.dump(state, open(os.environ["FAKE_ORCA_STATE"], "w"))


def registry_dir():
    return os.path.join(os.environ["XDG_STATE_HOME"], "tmux-tools", "orca", RUNTIME)


def open_pane(state, title, tab=None):
    number = state["next"]
    state["next"] = number + 1
    terminal = {
        "handle": "term_fake_%d" % number,
        "worktreeId": WORKTREE,
        "tabId": tab or "tab%d" % number,
        "leafId": "leaf%d" % number,
        "title": title,
        "paneTitle": title,
    }
    state["terminals"].append(terminal)
    return terminal


log = os.environ.get("FAKE_ORCA_LOG")
if log:
    with open(log, "a") as handle:
        handle.write(json.dumps(args) + "\n")

if os.environ.get("FAKE_ORCA_UNAVAILABLE") == "1":
    fail("runtime_unavailable", "Orca is not running (fake)")

state = load()

if path == ["status"]:
    answer({"runtime": {"reachable": True, "runtimeId": RUNTIME,
                        "capabilities": ["terminal.prompt-delivery.v1"]}})

if path == ["terminal", "list"] and os.environ.get("FAKE_ORCA_EMPTY_LISTS"):
    if state.get("empty_lists", 0) < int(os.environ["FAKE_ORCA_EMPTY_LISTS"]):
        state["empty_lists"] = state.get("empty_lists", 0) + 1
        save(state)
        sys.exit(0)

if path == ["terminal", "list"]:
    late = state.get("late")
    if late:
        late["lists"] -= 1
        if late["lists"] <= 0:
            state["late"] = None
            open_pane(state, late["title"], late.get("tab"))
        save(state)
    result = {"terminals": [dict(t) for t in state["terminals"]], "truncated": False,
              "hostScope": {"omittedHostIds": []}}
    name = os.environ.get("FAKE_ORCA_ON_LIST_SPAWN")
    if name and "on-list-spawn" not in state["flags"]:
        state["flags"].append("on-list-spawn")
        terminal = open_pane(state, name)
        save(state)
        os.makedirs(registry_dir(), exist_ok=True)
        json.dump({
            "schema": "tmux-tools.pane.v1", "name": name, "host": "orca",
            "server_id": RUNTIME, "handle": terminal["handle"], "agent": None,
            "access": None, "surface": None, "surface_unvalidated": False,
            "cwd": os.getcwd(), "launched_at": "2026-01-01T00:00:00Z",
        }, open(os.path.join(registry_dir(), name + ".json"), "w"))
    answer(result)

if path == ["terminal", "show"]:
    handle = flag("--terminal")
    for terminal in state["terminals"]:
        if terminal["handle"] == handle:
            answer({"terminal": terminal})
    fail("terminal_not_found", "no terminal " + str(handle))

if path == ["terminal", "create"] and os.environ.get("FAKE_ORCA_CREATE_LATE"):
    state["late"] = {"lists": int(os.environ["FAKE_ORCA_CREATE_LATE"]),
                     "title": flag("--title") or ""}
    save(state)
    fail("timeout", "terminal create timed out (fake)")

def split_tab():
    handle = flag("--terminal")
    for terminal in state["terminals"]:
        if terminal["handle"] == handle:
            return terminal["tabId"]
    fail("terminal_not_found", "no terminal " + str(handle))


if path == ["terminal", "split"] and os.environ.get("FAKE_ORCA_SPLIT_LATE"):
    state["late"] = {"lists": int(os.environ["FAKE_ORCA_SPLIT_LATE"]),
                     "title": "", "tab": split_tab()}
    save(state)
    fail("timeout", "terminal split timed out (fake)")

if path == ["terminal", "split"]:
    terminal = open_pane(state, "", split_tab())
    save(state)
    if os.environ.get("FAKE_ORCA_LOCK_REGISTRY") == "1":
        os.chmod(registry_dir(), 0o555)
    answer({"split": {"handle": terminal["handle"]}})

if path == ["terminal", "create"]:
    if os.environ.get("FAKE_ORCA_FOREIGN_ON_CREATE"):
        open_pane(state, os.environ["FAKE_ORCA_FOREIGN_ON_CREATE"])
    terminal = open_pane(state, flag("--title") or "")
    save(state)
    if os.environ.get("FAKE_ORCA_LOCK_REGISTRY") == "1":
        os.chmod(registry_dir(), 0o555)
    answer({"terminal": {"handle": terminal["handle"]}})

if path == ["terminal", "close"]:
    if os.environ.get("FAKE_ORCA_LOCK_REGISTRY") == "1":
        os.chmod(registry_dir(), 0o755)
    if os.environ.get("FAKE_ORCA_CLOSE_FAIL") == "1":
        fail("close_failed", "cannot close (fake)")
    handle = flag("--terminal")
    state["terminals"] = [t for t in state["terminals"] if t["handle"] != handle]
    save(state)
    answer({"closed": True})

if path == ["worktree", "ps"]:
    answer({"worktrees": []})

if path == ["terminal", "read"]:
    scrollback = int(os.environ.get("FAKE_ORCA_SCROLLBACK", "0"))
    lines = ["line%d" % n for n in range(1, scrollback + 1)]
    if "--screen" in args:
        lines = lines[-5:]
    elif flag("--limit"):
        lines = lines[-int(flag("--limit")):]
    answer({"terminal": {"tail": lines}})

if path == ["terminal", "send"]:
    answer({"send": {"accepted": True}})

fail("unknown_command", "fake orca does not implement " + " ".join(path))
