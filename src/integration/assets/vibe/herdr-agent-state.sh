#!/bin/sh
# installed by herdr
# managed by herdr; reinstalling or updating the integration overwrites this file.
# add custom hooks beside this file instead of editing it.
# HERDR_INTEGRATION_ID=vibe
# HERDR_INTEGRATION_VERSION=1

set -eu

hook_input_file="$(mktemp "${TMPDIR:-/tmp}/herdr-vibe-hook.XXXXXX")" || exit 0
trap 'rm -f "$hook_input_file"' EXIT HUP INT TERM
cat >"$hook_input_file" 2>/dev/null || true

[ "${HERDR_ENV:-}" = "1" ] || exit 0
[ -n "${HERDR_SOCKET_PATH:-}" ] || exit 0
[ -n "${HERDR_PANE_ID:-}" ] || exit 0
command -v python3 >/dev/null 2>&1 || exit 0

HERDR_HOOK_INPUT_FILE="$hook_input_file" python3 - 2>/dev/null <<'PY'
import json
import os
import socket
import time

source = "herdr:vibe"
pane_id = os.environ.get("HERDR_PANE_ID")
socket_path = os.environ.get("HERDR_SOCKET_PATH")
hook_input_file = os.environ.get("HERDR_HOOK_INPUT_FILE")
if not pane_id or not socket_path or not hook_input_file:
    raise SystemExit(0)

try:
    with open(hook_input_file, encoding="utf-8") as handle:
        hook_input = json.load(handle)
except Exception:
    raise SystemExit(0)
if not isinstance(hook_input, dict):
    raise SystemExit(0)
if hook_input.get("hook_event_name") != "post_agent":
    raise SystemExit(0)

session_id = hook_input.get("session_id")
if not isinstance(session_id, str) or not session_id.strip():
    raise SystemExit(0)
session_id = session_id.strip()

parent_session_id = hook_input.get("parent_session_id")
if isinstance(parent_session_id, str):
    if parent_session_id.strip():
        raise SystemExit(0)
elif parent_session_id is not None:
    raise SystemExit(0)

transcript_path = hook_input.get("transcript_path")
if not isinstance(transcript_path, str) or not transcript_path.strip():
    transcript_path = None
else:
    transcript_path = transcript_path.strip()

report_seq = time.time_ns()
params = {
    "pane_id": pane_id,
    "source": source,
    "agent": "vibe",
    "seq": report_seq,
    "agent_session_id": session_id,
}
if transcript_path:
    params["agent_session_path"] = transcript_path
request = {
    "id": f"{source}:{report_seq}",
    "method": "pane.report_agent_session",
    "params": params,
}

try:
    client = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    client.settimeout(0.5)
    client.connect(socket_path)
    client.sendall((json.dumps(request) + "\n").encode())
    try:
        client.recv(4096)
    except Exception:
        pass
    client.close()
except Exception:
    pass
PY
