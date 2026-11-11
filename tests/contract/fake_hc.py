#!/usr/bin/env python3
"""A stateful stand-in for the herdr-core `hc` CLI, used by the default suite's Herdr
tests (PRD-261109-2223-01 P8.2). It never reaches a real core.

Its terminals live in the JSON file FAKE_HC_STATE:
{"next": <n>, "terminals": [{"address", "name", "cwd", "argv", "labels", "state",
"status", "status_ms", "turn", "rows", "lines", "how", "complete", "keys",
"sent", "on_send"}]}. `hc get` reports the terminal's `rows` (default 24).

- FAKE_HC_LOG: append each call's argv as one JSON line.
- FAKE_HC_DOWN=1: `server status` fails.
- FAKE_HC_REJECT_KEYS=<a,b>: `keys` rejects these key names as `invalid_params`.

A terminal's `on_send` (set by a test) is applied by `send` after the text goes in:
`turn` adds to the turn, `status` sets the status word with `status_ms` now, `rewrite`
replaces its whole history, `append` adds lines to its history, `complete` sets the
`complete` value later reads report, `how` sets the `how` value later reads report. A terminal whose status word is `blocked` refuses `send`.
"""
import json
import os
import sys
import time

argv = sys.argv[1:]
if os.environ.get("FAKE_HC_LOG"):
    with open(os.environ["FAKE_HC_LOG"], "a") as log:
        log.write(json.dumps(argv) + "\n")

STATE = os.environ["FAKE_HC_STATE"]


def load():
    try:
        with open(STATE) as handle:
            return json.load(handle)
    except FileNotFoundError:
        return {"next": 1, "terminals": []}


def save(state):
    with open(STATE + ".tmp", "w") as handle:
        json.dump(state, handle)
    os.replace(STATE + ".tmp", STATE)


def answer(value):
    sys.stdout.write(json.dumps(value) + "\n")
    sys.exit(0)


def fail(kind, message, code):
    sys.stderr.write(json.dumps({"error": {"kind": kind, "message": message, "hint": ""}}) + "\n")
    sys.exit(code)


if "--json" not in argv:
    fail("usage", "the fake hc needs --json", 2)
split = argv.index("--json")
command = argv[:split]
rest = argv[split + 1:]
positional = []
options = {}
labels = {}
data = None
index = 0
while index < len(rest):
    arg = rest[index]
    if arg == "--":
        data = rest[index + 1:]
        break
    if arg == "--label":
        key, _, value = rest[index + 1].partition("=")
        labels[key] = value
        index += 2
        continue
    if arg in ("--name", "--cwd", "--lines"):
        options[arg] = rest[index + 1]
        index += 2
        continue
    if arg.startswith("--"):
        options[arg] = True
        index += 1
        continue
    positional.append(arg)
    index += 1

state = load()


def find(address):
    for terminal in state["terminals"]:
        if terminal["address"] == address:
            return terminal
    fail("not_found", "no terminal \"%s\"" % address, 3)


def full(terminal):
    out = {key: terminal.get(key) for key in (
        "address", "name", "cwd", "argv", "labels", "state", "status", "status_ms", "turn")}
    out["rows"] = terminal.get("rows", 24)
    out["status"] = {"state": terminal["status"]} if terminal.get("status") else None
    return out


def now_ms():
    return int(time.time() * 1000)


if command == ["server", "status"]:
    if os.environ.get("FAKE_HC_DOWN") == "1":
        fail("unavailable", "no core", 1)
    answer({"core": {"id": "fake-core", "name": "fake"}, "version": "0.1.19"})

if command == ["run"]:
    name = options.get("--name")
    if name and any(t["name"] == name and t["state"] == "live" for t in state["terminals"]):
        fail("name_taken", "a live terminal is already named \"%s\"" % name, 1)
    address = "t%d" % state["next"]
    state["next"] += 1
    terminal = {
        "address": address, "name": name, "cwd": options.get("--cwd"),
        "argv": ["sh", "-c", (data or [""])[0]], "labels": labels, "state": "live",
        "status": None, "status_ms": None, "turn": 0, "lines": [], "how": "history",
        "complete": True, "keys": [], "sent": [], "on_send": {},
    }
    state["terminals"].append(terminal)
    save(state)
    answer({"address": address, "agent": "sh", "cwd": terminal["cwd"], "name": name,
            "state": "live", "turn": 0})

if command == ["ls"]:
    rows = []
    for terminal in state["terminals"]:
        row = {key: terminal.get(key) for key in ("address", "cwd", "name", "state", "turn")}
        if "where" in terminal["labels"]:
            row["where"] = terminal["labels"]["where"]
        rows.append(row)
    answer({"terminals": rows, "hidden_finished": 0})

if command == ["get"]:
    answer(full(find(positional[0])))

if command == ["read"]:
    terminal = find(positional[0])
    answer({"text": "\n".join(terminal["lines"]), "how": terminal.get("how", "history"),
            "complete": terminal.get("complete", True), "why": None})

if command == ["keys"]:
    terminal = find(positional[0])
    rejected = [k for k in os.environ.get("FAKE_HC_REJECT_KEYS", "").split(",") if k]
    for key in data or []:
        if key in rejected:
            fail("invalid_params", "unknown key \"%s\"" % key, 6)
    terminal["keys"].extend(data or [])
    save(state)
    answer({"status": terminal["status"], "turn": terminal["turn"]})

if command == ["send"]:
    terminal = find(positional[0])
    if terminal.get("status") == "blocked":
        fail("blocked", "it is blocked: permission", 9)
    text = (data or [""])[0]
    terminal["sent"].append(text)
    terminal["lines"].extend(text.split("\n"))
    effect = terminal.get("on_send") or {}
    terminal["turn"] += effect.get("turn", 0)
    if "status" in effect:
        terminal["status"] = effect["status"]
        terminal["status_ms"] = now_ms()
    if "rewrite" in effect:
        terminal["lines"] = list(effect["rewrite"])
    terminal["lines"].extend(effect.get("append", []))
    if "complete" in effect:
        terminal["complete"] = effect["complete"]
    if "how" in effect:
        terminal["how"] = effect["how"]
    save(state)
    answer({"started": None, "status": terminal["status"], "turn": terminal["turn"]})

if command == ["label"]:
    terminal = find(positional[0])
    for pair in data or []:
        key, _, value = pair.partition("=")
        if value:
            terminal["labels"][key] = value
        else:
            terminal["labels"].pop(key, None)
    save(state)
    answer(full(terminal))

if command == ["kill"]:
    terminal = find(positional[0])
    terminal["state"] = "finished"
    terminal["status"] = None
    save(state)
    answer(full(terminal))

if command == ["rm"]:
    terminal = find(positional[0])
    state["terminals"].remove(terminal)
    save(state)
    answer({"removed": terminal["address"]})

fail("usage", "the fake hc does not serve %s" % command, 2)
