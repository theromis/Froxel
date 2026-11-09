# SPDX-License-Identifier: MIT
"""Reload a running `froxel mcp` in place and check the client keeps its
connection, its document, and hears `notifications/tools/list_changed`.

Runs `target/debug/froxel` (or FROXEL_BIN): `cargo build && python3
tests/mcp_reload.py`. Unix only, as reload is.
"""

import json
import os
import pathlib
import shutil
import subprocess
import tempfile
import time

ROOT = pathlib.Path(__file__).resolve().parents[1]
BIN = pathlib.Path(os.environ.get("FROXEL_BIN", ROOT / "target" / "debug" / "froxel"))
LIST_CHANGED = "notifications/tools/list_changed"


class Client:
    def __init__(self, binary, *args):
        self.proc = subprocess.Popen(
            [str(binary), "mcp", *args],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
        )
        self.next_id = 1

    def send(self, *messages):
        """Write several messages in one write, as a pipelining client may."""
        self.proc.stdin.write("".join(json.dumps(m) + "\n" for m in messages))
        self.proc.stdin.flush()

    def request(self, method, params=None):
        msg = {"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params or {}}
        self.next_id += 1
        return msg

    def call(self, name, arguments=None):
        return self.request("tools/call", {"name": name, "arguments": arguments or {}})

    def read(self):
        line = self.proc.stdout.readline()
        if not line:
            raise SystemExit(f"server closed stdout (exit {self.proc.poll()}): {self.proc.stderr.read()}")
        return json.loads(line)

    def result(self, reply):
        assert "result" in reply, reply
        content = reply["result"]
        if "structuredContent" in content:
            return content["structuredContent"]
        return json.loads(content["content"][0]["text"])

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=10)


def tool_names(client):
    client.send(client.request("tools/list"))
    return {tool["name"] for tool in client.read()["result"]["tools"]}


def reload_tool():
    client = Client(BIN)
    client.send(client.request("initialize", {"protocolVersion": "2025-06-18"}))
    init = client.read()["result"]
    assert init["capabilities"]["tools"]["listChanged"] is True, init
    client.send({"jsonrpc": "2.0", "method": "notifications/initialized"})
    assert "reload" in tool_names(client)
    client.send(client.call("set_voxel", {"x": 2, "y": 3, "z": 4, "palette": 9}))
    assert client.result(client.read())["changed"] == 1
    pid = client.proc.pid

    client.send(client.call("reload"))
    unchanged = client.result(client.read())
    assert unchanged["reloaded"] is False, unchanged

    # The reload and the next request in one write: the request read past
    # the reload is handed to the new process and answered by it.
    client.send(client.call("reload", {"force": True}), client.call("get_voxel", {"x": 2, "y": 3, "z": 4}))
    done = client.result(client.read())
    assert done["reloaded"] is True, done
    note = client.read()
    assert note == {"jsonrpc": "2.0", "method": LIST_CHANGED}, note
    voxel = client.result(client.read())
    assert voxel["palette"] == 9, voxel
    assert client.proc.pid == pid and client.proc.poll() is None
    assert "reload" in tool_names(client)
    client.close()
    print("reload: kept the connection and the voxel, sent", LIST_CHANGED)


def watch():
    folder = pathlib.Path(tempfile.mkdtemp(prefix="froxel-reload-"))
    try:
        binary = folder / "froxel"
        shutil.copy2(BIN, binary)
        client = Client(binary, "--watch")
        client.send(client.request("initialize", {}))
        client.read()
        client.send(client.call("set_voxel", {"x": 0, "y": 0, "z": 0, "palette": 5}))
        client.read()
        # A rebuild writes a new file over the old one, as the linker does.
        time.sleep(1.1)
        fresh = folder / "froxel.new"
        shutil.copy(BIN, fresh)
        os.replace(fresh, binary)
        client.send(client.call("get_voxel", {"x": 0, "y": 0, "z": 0}))
        note = client.read()
        assert note == {"jsonrpc": "2.0", "method": LIST_CHANGED}, note
        voxel = client.result(client.read())
        assert voxel["palette"] == 5, voxel
        # No change since: no second reload.
        client.send(client.call("get_voxel", {"x": 0, "y": 0, "z": 0}))
        assert "result" in client.read()
        client.close()
        print("watch: reloaded before the first request after the rebuild")
    finally:
        shutil.rmtree(folder, ignore_errors=True)


def refused():
    folder = pathlib.Path(tempfile.mkdtemp(prefix="froxel-reload-"))
    try:
        binary = folder / "froxel"
        shutil.copy2(BIN, binary)
        client = Client(binary)
        client.send(client.request("initialize", {}))
        client.read()
        # The rebuild is broken: the server refuses it and keeps running.
        broken = folder / "froxel.new"
        broken.write_text("#!/bin/sh\nexit 1\n")
        broken.chmod(0o755)
        os.replace(broken, binary)
        client.send(client.call("reload"), client.call("inspect_document"))
        reply = client.read()
        assert reply["result"]["isError"] is True, reply
        assert "keeps running" in reply["result"]["content"][0]["text"], reply
        assert "result" in client.read()
        client.close()
        print("refused: a broken binary is not run")
    finally:
        shutil.rmtree(folder, ignore_errors=True)


def main():
    if not BIN.is_file():
        raise SystemExit(f"missing binary: {BIN}")
    if os.name != "posix":
        print("skipped: reload needs Unix")
        return
    reload_tool()
    watch()
    refused()
    print("OK")


if __name__ == "__main__":
    main()
