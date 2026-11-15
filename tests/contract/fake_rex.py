#!/usr/bin/env python3
"""A stateful stand-in for the `rex` CLI, used by the default suite's Rex tests
(PRD-261112-2108-01 P8.2). It never reaches a real Rex server.

Its servers live in the JSON file FAKE_REX_STATE:
{"next": <n>, "servers": [{"addresses", "instance_id", "api_revision", "down",
"status_sleep", "fail_ls", "fail_block_ls", "create", "sessions", "blocks"}]}.

A call reaches the server whose `addresses` holds the `-S` value, else the
`REX_SERVER` value from the environment, else `default`. No such server, or a server
with `down`, fails every call with exit 1 (as `rex` does when no server runs).
`status_sleep` delays `server status` by that many seconds. `ls_sleep` delays `ls`, and
`ls_garbage` makes it answer output that does not parse; `block_ls_sleep` and
`block_ls_garbage` list the sessions whose `block ls` sleeps 5 seconds or answers
output that does not parse. `on_block_ls` ({"block", "records"}) adds those records,
stamped now, to that block during the first `block ls` after a `program_status` read,
and is then cleared.

A block is {"block_id", "session_id", "window_id", "label", "argv", "cwd", "lines",
"rows", "columns", "alternate", "modes", "records", "sent", "pasted", "keys",
"on_send"}. `modes` lists the DEC private modes that are on (1, 2004, ...). A record is
{"state", "updated_at"}.

A server's `create` sets how the next `run` or `split` ends: {"exit": <code>,
"make": <bool>, "garbage": <bool>}; the default makes the block and answers its JSON.

`api call block.event` with a `paste` event adds the text to `pasted` and its lines to
the block's `lines`. `send-key enter` then applies the block's `on_send` (set by a
test): `records` adds records stamped now, `append` adds lines, `rewrite` replaces all
lines, `drop` removes that many oldest lines, `rows`, `columns`, `alternate` set them.

- FAKE_REX_LOG: append each call's argv as one JSON line.
"""
import datetime
import json
import os
import sys
import time

argv = sys.argv[1:]
if os.environ.get("FAKE_REX_LOG"):
    with open(os.environ["FAKE_REX_LOG"], "a") as log:
        log.write(json.dumps(argv) + "\n")

STATE = os.environ["FAKE_REX_STATE"]


def load():
    with open(STATE) as handle:
        return json.load(handle)


def save(state):
    with open(STATE + ".tmp", "w") as handle:
        json.dump(state, handle)
    os.replace(STATE + ".tmp", STATE)


def answer(value):
    sys.stdout.write(json.dumps(value) + "\n")
    sys.exit(0)


def fail(message, code):
    sys.stderr.write("Error: %s\n" % message)
    sys.exit(code)


def now_rfc3339():
    return datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")


# Global flags, anywhere before the command's own `--`.
address = None
words = []
index = 0
while index < len(argv):
    arg = argv[index]
    if arg == "--":
        words.extend(argv[index:])
        break
    if arg.startswith("--autostart"):
        index += 1
        continue
    if arg == "-S":
        address = argv[index + 1]
        index += 2
        continue
    if arg == "--timeout":
        index += 2
        continue
    words.append(arg)
    index += 1

if address is None:
    address = os.environ.get("REX_SERVER") or "default"

state = load()
server = next((s for s in state["servers"] if address in s.get("addresses", [])), None)
if server is None or server.get("down"):
    fail("rex server is not running", 1)

# The command words, then its options.
command = []
rest = list(words)
while rest and not rest[0].startswith("-"):
    command.append(rest.pop(0))
if command[:2] == ["block", "call"] or command[:2] == ["api", "call"]:
    pass
options = {}
positional = []
data = None
index = 0
while index < len(rest):
    arg = rest[index]
    if arg == "--":
        data = rest[index + 1:]
        break
    if arg.startswith("--") and "=" in arg:
        key, _, value = arg.partition("=")
        options[key] = value
        index += 1
        continue
    if arg in ("-b", "-s", "-w", "--label", "--cwd", "--split", "--block", "--session"):
        options[arg] = rest[index + 1]
        index += 2
        continue
    if arg.startswith("-"):
        options[arg] = True
        index += 1
        continue
    positional.append(arg)
    index += 1

# Positional words after the command words (`block close <id>`, `block call <method>`).
if len(command) > 2 and command[0] in ("block", "api"):
    positional = command[2:] + positional
    command = command[:2]


def find(block_id):
    for block in server["blocks"]:
        if block["block_id"] == block_id:
            return block
    fail('block target "%s" not found' % block_id, 3)


def new_id(kind):
    number = state["next"]
    state["next"] = number + 1
    return "%s:00000000-0000-4000-8000-%012d" % (kind, number)


if command == ["server", "status"]:
    time.sleep(server.get("status_sleep", 0))
    answer({"endpoint": address, "instance_id": server["instance_id"],
            "api_revision": server.get("api_revision", 13)})

def garbage():
    sys.stdout.write("not json\n")
    sys.exit(0)


if command == ["ls"]:
    if server.get("fail_ls"):
        fail("listing sessions failed", 1)
    time.sleep(server.get("ls_sleep", 0))
    if server.get("ls_garbage"):
        garbage()
    answer({"sessions": server["sessions"]})

if command == ["block", "ls"]:
    session = options.get("-s")
    if session in server.get("fail_block_ls", []):
        fail("listing blocks failed", 1)
    if session in server.get("block_ls_sleep", []):
        time.sleep(5)
    if session in server.get("block_ls_garbage", []):
        garbage()
    publish = server.get("on_block_ls")
    if publish and server.get("status_read"):
        target = find(publish["block"])
        stamp = now_rfc3339()
        for record in publish.get("records", []):
            target["records"].append({"state": record, "updated_at": stamp})
        server["on_block_ls"] = None
        save(state)
    if not any(s["session_id"] == session for s in server["sessions"]):
        fail('session "%s" not found' % session, 3)
    rows = []
    for block in server["blocks"]:
        if block["session_id"] == session:
            rows.append({"block_id": block["block_id"], "label": block.get("label", ""),
                         "window_id": block.get("window_id"), "placement": "placed"})
    answer({"blocks": rows})

if command in (["run"], ["split"]):
    if command == ["split"]:
        anchor = find(options.get("-b"))
        session, window = anchor["session_id"], anchor["window_id"]
    else:
        session = options.get("-s")
        if not any(s["session_id"] == session for s in server["sessions"]):
            fail('session "%s" not found' % session, 3)
        window = new_id("window")
    behavior = server.get("create") or {}
    block_id = None
    if behavior.get("make", True):
        block_id = new_id("block")
        server["blocks"].append({
            "block_id": block_id, "session_id": session, "window_id": window,
            "label": options.get("--label", ""), "argv": data or [], "cwd": options.get("--cwd"),
            "lines": [], "rows": 24, "columns": 80, "alternate": False, "modes": [],
            "records": [], "sent": [], "pasted": [], "keys": [], "on_send": {},
        })
    save(state)
    code = behavior.get("exit", 0)
    if code != 0:
        fail("session.new_window request timed out after 10s" if code == 1 else "create failed", code)
    if behavior.get("garbage"):
        sys.stdout.write("not json\n")
        sys.exit(0)
    if command == ["split"]:
        answer({"anchor_id": options.get("-b"), "block_ids": [block_id], "revision": 2})
    answer({"window_id": window, "block_ids": [block_id], "revision": 2})

if command == ["capture"]:
    block = find(options.get("-b"))
    sys.stdout.write("".join(line + "\n" for line in block["lines"]) or "\n")
    sys.exit(0)

if command == ["block", "call"]:
    block = find(options.get("-b"))
    method = positional[0]
    if method == "size":
        answer({"columns": block.get("columns", 80), "rows": block.get("rows", 24)})
    if method == "modes":
        on = set(block.get("modes", []))
        modes = [{"name": "m%d" % n, "number": n, "set": n in on} for n in (1, 25, 1049, 2004)]
        answer({"screen": "alternate" if block.get("alternate") else "primary", "modes": modes})
    if method == "program_status":
        if server.get("on_block_ls") and not server.get("status_read"):
            server["status_read"] = True
            save(state)
        answer({"records": block.get("records", [])})
    fail("unknown method %s" % method, 2)

if command == ["send"]:
    block = find(options.get("-b"))
    text = (data if data is not None else positional)[0]
    block["sent"].append(text)
    save(state)
    sys.exit(0)

if command == ["send-key"]:
    block = find(options.get("-b"))
    block["keys"].extend(positional)
    effect = block.get("on_send") or {}
    if "enter" in positional and effect:
        stamp = now_rfc3339()
        for record in effect.get("records", []):
            block["records"].append({"state": record, "updated_at": stamp})
        if "rewrite" in effect:
            block["lines"] = list(effect["rewrite"])
        block["lines"].extend(effect.get("append", []))
        block["lines"] = block["lines"][effect.get("drop", 0):]
        for key in ("rows", "columns", "alternate"):
            if key in effect:
                block[key] = effect[key]
        block["on_send"] = {}
    save(state)
    sys.exit(0)

if command == ["api", "call"]:
    method = positional[0]
    request = json.loads(positional[1])
    if method != "block.event":
        fail("unknown method %s" % method, 2)
    block = find(request["block_id"])
    if request.get("session_id") != block["session_id"]:
        fail("session mismatch", 3)
    event = request["event"]
    if event["type"] == "paste":
        text = event["payload"]["text"]
        block["pasted"].append(text)
        block["lines"].extend(text.split("\n"))
    save(state)
    answer({})

if command == ["block", "close"]:
    block = find(positional[0])
    server["blocks"].remove(block)
    save(state)
    sys.exit(0)

fail("the fake rex does not serve %s" % command, 2)
