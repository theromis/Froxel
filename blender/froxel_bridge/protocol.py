# SPDX-License-Identifier: MIT
"""Froxel bridge frames and session checks. No Blender dependency."""

import json
import os

MAX_FRAME = 65536


class Frames:
    def __init__(self):
        self.buffer = b""

    def feed(self, data):
        self.buffer += data
        lines = self.buffer.split(b"\n")
        self.buffer = lines.pop()
        if len(self.buffer) > MAX_FRAME or any(len(line) > MAX_FRAME for line in lines):
            raise ValueError("frame too large")
        messages = []
        for line in lines:
            if line.endswith(b"\r"):
                line = line[:-1]
            if not line:
                continue
            messages.append(json.loads(line.decode("utf-8")))
        return messages


def encode(value):
    return (json.dumps(value, allow_nan=False, separators=(",", ":")) + "\n").encode()


def load_session(text):
    data = json.loads(text)
    if (
        not isinstance(data, dict)
        or data.get("v") != 1
        or data.get("host") != "127.0.0.1"
        or type(data.get("port")) is not int
        or not 0 < data["port"] < 65536
        or not isinstance(data.get("token"), str)
        or not data["token"]
        or not isinstance(data.get("root"), str)
        or not data["root"]
    ):
        raise ValueError("invalid loopback session file")
    return data


def accept_result(session, path):
    """Return the result path when it is a glb directly inside the session root."""
    root = os.path.abspath(session["root"])
    file = os.path.abspath(path)
    if os.path.dirname(file) != root or not file.endswith(".glb"):
        raise ValueError("result path outside session directory")
    return file
