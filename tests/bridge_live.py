# SPDX-License-Identifier: MIT
"""Talk to `froxel bridge` and check that the frog comes back as a glb."""

import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "blender" / "froxel_bridge"))
import protocol  # noqa: E402

BIN = pathlib.Path(os.environ.get("FROXEL_BIN", ROOT / "target" / "debug" / "froxel"))
VOX = ROOT / "examples" / "frog.vox"


def main():
    if not BIN.is_file():
        raise SystemExit(f"missing binary: {BIN}")
    proc = subprocess.Popen(
        [str(BIN), "bridge", "--input", str(VOX), "--port", "0"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    session_line = ""
    assert proc.stdout is not None
    for _ in range(20):
        line = proc.stdout.readline()
        if not line:
            raise SystemExit(f"bridge exited {proc.poll()} before FROXEL_SESSION")
        if line.startswith("FROXEL_SESSION="):
            session_line = line.strip()
            break
    if not session_line:
        proc.kill()
        raise SystemExit("no FROXEL_SESSION line")
    try:
        session = protocol.load_session(pathlib.Path(session_line.split("=", 1)[1]).read_text())
        import socket

        sock = socket.create_connection(("127.0.0.1", session["port"]), timeout=10)
        sock.settimeout(30)
        frames = protocol.Frames()

        def call(op, extra=None):
            body = {"v": 1, "id": call.next, "token": session["token"], "op": op}
            call.next += 1
            if extra:
                body.update(extra)
            sock.sendall(protocol.encode(body))
            while True:
                data = sock.recv(65536)
                if not data:
                    raise SystemExit("bridge closed")
                messages = frames.feed(data)
                if messages:
                    return messages[0]

        call.next = 1
        schema = call("describe")
        if schema.get("op") != "schema":
            raise SystemExit(f"describe failed: {schema}")
        result = call("evaluate", {"values": {}})
        if result.get("op") != "result":
            raise SystemExit(f"evaluate failed: {result}")
        path = protocol.accept_result(session, result["path"])
        header = pathlib.Path(path).read_bytes()[:4]
        if header != b"glTF":
            raise SystemExit(f"not a glb: {header!r}")
        print(path)
    finally:
        # SIGTERM, not SIGKILL, so the bridge removes its session directory.
        proc.terminate()
        proc.wait(timeout=5)


if __name__ == "__main__":
    main()
