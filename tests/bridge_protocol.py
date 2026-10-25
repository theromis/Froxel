# SPDX-License-Identifier: MIT
"""Client codec for the Froxel bridge. The server rules live in the Rust tests."""

import json
import unittest
from pathlib import Path
import importlib.util

SPEC = importlib.util.spec_from_file_location(
    "froxel_protocol",
    Path(__file__).resolve().parents[1] / "blender" / "froxel_bridge" / "protocol.py",
)
protocol = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(protocol)


class ProtocolTests(unittest.TestCase):
    def test_fragmented_utf8_and_coalesced_frames(self):
        wire = protocol.encode({"name": "Этаж"}) + protocol.encode({"id": 2})
        frames = protocol.Frames()
        got = []
        for byte in wire:
            got.extend(frames.feed(bytes([byte])))
        self.assertEqual(got, [{"name": "Этаж"}, {"id": 2}])

    def test_frame_bounds(self):
        frames = protocol.Frames()
        with self.assertRaises(ValueError):
            frames.feed(b"x" * (protocol.MAX_FRAME + 1))
        with self.assertRaises(ValueError):
            frames.feed(b"x" * (protocol.MAX_FRAME + 1) + b"\n")

    def test_session_file(self):
        good = {"v": 1, "host": "127.0.0.1", "port": 29771, "token": "abc", "root": "/tmp/froxel"}
        self.assertEqual(protocol.load_session(json.dumps(good)), good)
        for broken in (
            {**good, "host": "10.0.0.1"},
            {**good, "v": 2},
            {**good, "port": 0},
            {**good, "token": ""},
        ):
            with self.assertRaises(ValueError):
                protocol.load_session(json.dumps(broken))

    def test_result_path_stays_in_the_session(self):
        session = {"root": "/tmp/froxel-bridge"}
        self.assertTrue(
            protocol.accept_result(session, "/tmp/froxel-bridge/result-1.glb").endswith(".glb")
        )
        for path in (
            "/tmp/froxel-bridge/nested/result-1.glb",
            "/tmp/froxel-bridge-other/result-1.glb",
            "/tmp/froxel-bridge/result-1.gltf",
            "/etc/passwd",
        ):
            with self.assertRaises(ValueError):
                protocol.accept_result(session, path)


if __name__ == "__main__":
    unittest.main()
