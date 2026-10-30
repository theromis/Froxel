# SPDX-License-Identifier: MIT
"""Bake through the Godot FroxelBridge node and check the scenes with the bridge stopped.

Needs Godot 4.3+ in GODOT_PATH (skipped without it) and a froxel binary (FROXEL_BIN, default
target/debug/froxel). The example project in godot/ is copied to a temporary folder, so the
repository is not touched.
"""

import json
import os
import pathlib
import shutil
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
BIN = pathlib.Path(os.environ.get("FROXEL_BIN", ROOT / "target" / "debug" / "froxel"))
VOX = ROOT / "examples" / "lily_pad.vox"

# Bakes once from a node placed in a scene: the node's transform is kept when asked.
BAKE_PLACED = """extends SceneTree

func _initialize() -> void:
\tcall_deferred("run")

func run() -> void:
\tvar session := OS.get_cmdline_user_args()[0]
\tvar bridge: Node3D = load("res://addons/froxel_bridge/bridge.gd").new()
\tbridge.position = Vector3(1, 2, 3)
\tbridge.rotation_degrees = Vector3(0, 90, 0)
\tbridge.bake_voxel_size = 0.04
\tbridge.bake_origin = 2
\tbridge.bake_apply_transform = true
\tbridge.bake_collision = 2
\troot.add_child(bridge)
\tbridge.session_file = session
\tbridge.connect_bridge()
\tvar error: String = await bridge.wait_schema()
\tif error == "":
\t\terror = await bridge.bake_to("res://out/moved.tscn")
\tif error != "":
\t\tpush_error(error)
\t\tquit(1)
\t\treturn
\tprint("BOUNDS_RAW=", JSON.stringify(aabb(bridge.voxel_bounds(bridge.preview))))
\tquit(0)

func aabb(box: AABB) -> Array:
\treturn [box.position.x, box.position.y, box.position.z, box.size.x, box.size.y, box.size.z]
"""

# Loads every baked scene without the bridge and reports what is in it.
CHECK = """extends SceneTree

func _initialize() -> void:
\tvar report := {}
\tfor name in ["raw", "placed", "capsule", "moved"]:
\t\tvar packed := load("res://out/%s.tscn" % name) as PackedScene
\t\tif packed == null:
\t\t\treport[name] = null
\t\t\tcontinue
\t\tvar scene := packed.instantiate() as Node3D
\t\tvar entry := {
\t\t\t"root": scene.name,
\t\t\t"root_transform": xform(scene.transform),
\t\t\t"children": [],
\t\t\t"bridge_script": has_bridge(scene),
\t\t}
\t\tfor child in scene.get_children():
\t\t\tentry["children"].append(child.name)
\t\tvar visual := scene.get_node_or_null("Visual") as Node3D
\t\tif visual != null:
\t\t\tentry["visual_transform"] = xform(visual.transform)
\t\t\tentry["bounds"] = aabb(visual.transform * bounds(visual))
\t\telse:
\t\t\tentry["bounds"] = aabb(bounds(scene))
\t\tvar shape := scene.get_node_or_null("Collision/Shape") as CollisionShape3D
\t\tif shape != null:
\t\t\tentry["shape_position"] = [shape.position.x, shape.position.y, shape.position.z]
\t\t\tif shape.shape is BoxShape3D:
\t\t\t\tvar size: Vector3 = (shape.shape as BoxShape3D).size
\t\t\t\tentry["box"] = [size.x, size.y, size.z]
\t\t\telif shape.shape is CapsuleShape3D:
\t\t\t\tvar capsule := shape.shape as CapsuleShape3D
\t\t\t\tentry["capsule"] = [capsule.radius, capsule.height]
\t\treport[name] = entry
\t\tscene.free()
\tprint("REPORT=", JSON.stringify(report))
\tquit(0)

func has_bridge(node: Node) -> bool:
\tif node.get_script() != null:
\t\treturn true
\tfor child in node.get_children():
\t\tif has_bridge(child):
\t\t\treturn true
\treturn false

func bounds(node: Node3D) -> AABB:
\tvar found := []
\tcollect(node, Transform3D.IDENTITY, found)
\tvar box: AABB = found[0]
\tfor item in found:
\t\tbox = box.merge(item)
\treturn box

func collect(node: Node, to_root: Transform3D, found: Array) -> void:
\tfor child in node.get_children():
\t\tif child is Node3D:
\t\t\tvar placed: Transform3D = to_root * child.transform
\t\t\tif child is MeshInstance3D and child.mesh != null:
\t\t\t\tfound.append(placed * child.mesh.get_aabb())
\t\t\tcollect(child, placed, found)

func xform(t: Transform3D) -> Array:
\treturn [t.basis.x.x, t.basis.x.y, t.basis.x.z, t.basis.y.x, t.basis.y.y, t.basis.y.z,
\t\tt.basis.z.x, t.basis.z.y, t.basis.z.z, t.origin.x, t.origin.y, t.origin.z]

func aabb(box: AABB) -> Array:
\treturn [box.position.x, box.position.y, box.position.z, box.size.x, box.size.y, box.size.z]
"""


# A script with a point list, for the list-parameter check.
LIST_SCRIPT = """let path = param_vec2s("path", [[0.0, 0.0], [3.0, 0.0]], 0.0, 12.0, 4);
for p in path {
    fill_box(to_int(p[0]), to_int(p[1]), 0, to_int(p[0]), to_int(p[1]), 2, 1);
}
"""

# The inspector type of the list parameter and what a set_parameter sends.
CHECK_LIST = """extends SceneTree

func _initialize() -> void:
\tcall_deferred("run")

func run() -> void:
\tvar bridge: Node3D = load("res://addons/froxel_bridge/bridge.gd").new()
\troot.add_child(bridge)
\tbridge.session_file = OS.get_cmdline_user_args()[0]
\tbridge.connect_bridge()
\tvar error: String = await bridge.wait_schema()
\tvar report := {"schema_error": error}
\tfor entry in bridge.get_property_list():
\t\tif str(entry.name).ends_with("/path"):
\t\t\treport["type"] = entry.type
\treport["value"] = typeof(bridge.values["0/path"])
\treport["set"] = bridge.set_parameter("path", [[1, 2], [5, 6.5]])
\treport["wire"] = bridge.wire_values()["0/path"]
\treport["long"] = bridge.set_parameter("path", [[0, 0], [0, 0], [0, 0], [0, 0], [0, 0]])
\treport["range"] = bridge.set_parameter("path", [[0, 13]])
\treport["shape"] = bridge.set_parameter("path", [1, 2])
\tprint("LIST=", JSON.stringify(report))
\tquit(0)
"""


def near(a, b, tolerance=1e-3):
    return abs(a - b) <= tolerance * max(1.0, abs(b))


def all_near(values, expected, tolerance=1e-3):
    return len(values) == len(expected) and all(near(a, b, tolerance) for a, b in zip(values, expected))


def main():
    godot = os.environ.get("GODOT_PATH", "")
    if not godot or not pathlib.Path(godot).is_file():
        print("skipped: set GODOT_PATH to a Godot 4.3+ executable")
        return
    if not BIN.is_file():
        raise SystemExit(f"missing binary: {BIN}")
    work = pathlib.Path(tempfile.mkdtemp(prefix="froxel-godot-bake-"))
    project = work / "project"
    shutil.copytree(ROOT / "godot", project)
    (project / "out").mkdir()
    (project / "bake_placed.gd").write_text(BAKE_PLACED)
    (project / "check_bake.gd").write_text(CHECK)
    variants = work / "variants.json"
    variants.write_text("[{}]")

    def run_godot(*args, check=True):
        done = subprocess.run(
            [godot, "--headless", "--path", str(project), *args],
            capture_output=True,
            text=True,
            timeout=300,
        )
        if check and done.returncode != 0:
            raise SystemExit(f"godot {' '.join(args)} exited {done.returncode}:\n{done.stdout}\n{done.stderr}")
        return done

    def line(output, prefix):
        for entry in output.splitlines():
            if entry.startswith(prefix):
                return json.loads(entry[len(prefix):])
        raise SystemExit(f"no {prefix} line in:\n{output}")

    run_godot("--import")
    bridge = subprocess.Popen(
        [str(BIN), "bridge", "--input", str(VOX), "--port", "0"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    try:
        session = ""
        assert bridge.stdout is not None
        for _ in range(20):
            entry = bridge.stdout.readline()
            if not entry:
                raise SystemExit(f"bridge exited {bridge.poll()} before FROXEL_SESSION")
            if entry.startswith("FROXEL_SESSION="):
                session = entry.strip().split("=", 1)[1]
                break
        if not session:
            raise SystemExit("no FROXEL_SESSION line")

        def bake(out, *extra, check=True):
            return run_godot(
                "--script", "res://addons/froxel_bridge/bake.gd", "--",
                "--session", session, "--out", out, "--variants", str(variants), *extra,
                check=check,
            )

        bake("res://out/raw.tscn")
        bake("res://out/placed.tscn", "--voxel-size", "0.04", "--origin", "bottom_center", "--collision", "box")
        bake("res://out/capsule.tscn", "--voxel-size", "0.04", "--origin", "center", "--collision", "capsule")
        placed_raw = line(run_godot("--script", "res://bake_placed.gd", "--", session).stdout, "BOUNDS_RAW=")

        # A failed bake keeps the previous scene: a bad option fails before writing, and a
        # staging file that cannot be written fails before the old scene is replaced.
        placed = project / "out" / "placed.tscn"
        before = placed.read_bytes()
        refused = bake("res://out/placed.tscn", "--origin", "middle", check=False)
        assert refused.returncode != 0, refused.stdout
        (project / "out" / "placed.baking.tscn").mkdir()
        blocked = bake("res://out/placed.tscn", "--voxel-size", "0.08", "--collision", "box", check=False)
        assert blocked.returncode != 0, blocked.stdout
        assert placed.read_bytes() == before, "a failed bake replaced the previous scene"
        (project / "out" / "placed.baking.tscn").rmdir()
    finally:
        bridge.terminate()
        bridge.wait(timeout=10)

    # The bridge is stopped: every scene must load and stand on its own.
    report = line(run_godot("--script", "res://check_bake.gd").stdout, "REPORT=")
    identity = [1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]

    raw = report["raw"]
    assert raw is not None and not raw["bridge_script"], raw
    assert "Visual" not in raw["children"] and "Collision" not in raw["children"], raw
    rx, ry, rz, sx, sy, sz = raw["bounds"]
    assert sx > 1 and sz > 1, f"raw bake is not in voxel units: {raw}"

    unit = 0.04
    entry = report["placed"]
    assert entry["children"] == ["Visual", "Collision"], entry
    assert all_near(entry["root_transform"], identity), entry
    x, y, z, w, h, d = entry["bounds"]
    assert all_near([w, h, d], [sx * unit, sy * unit, sz * unit]), (entry, raw)
    assert near(y, 0.0) and near(x + w / 2, 0.0) and near(z + d / 2, 0.0), f"origin is not bottom center: {entry}"
    assert all_near(entry["box"], [w, h, d]), entry
    assert all_near(entry["shape_position"], [x + w / 2, y + h / 2, z + d / 2]), entry

    entry = report["capsule"]
    x, y, z, w, h, d = entry["bounds"]
    assert all_near([x + w / 2, y + h / 2, z + d / 2], [0, 0, 0]), f"origin is not center: {entry}"
    radius, height = entry["capsule"]
    assert near(radius, max(w, d) / 2) and near(height, max(h, 2 * radius)), entry

    # The node at (1, 2, 3), turned 90 degrees about Y: the bounds keep that placement.
    entry = report["moved"]
    x, y, z, w, h, d = entry["bounds"]
    _, _, _, mw, mh, md = placed_raw
    assert all_near([w, h, d], [md * unit, mh * unit, mw * unit]), (entry, placed_raw)
    assert near(y, 2.0) and near(x + w / 2, 1.0) and near(z + d / 2, 3.0), f"node transform not kept: {entry}"
    assert "capsule" in entry, entry

    check_lists(work, project, run_godot, line)

    shutil.rmtree(work, ignore_errors=True)
    print("ok: raw, placed, capsule and moved bakes; a failed bake kept the previous scene;")
    print("ok: a vec2s parameter is a PackedVector2Array, bakes from variants.json, and is refused too long")


def make_list_vox(path):
    """A .vox whose script declares a vec2s parameter, written by `froxel mcp`."""
    calls = [
        {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}},
        {"jsonrpc": "2.0", "id": 2, "method": "tools/call",
         "params": {"name": "set_script", "arguments": {"source": LIST_SCRIPT}}},
        {"jsonrpc": "2.0", "id": 3, "method": "tools/call",
         "params": {"name": "regenerate", "arguments": {}}},
        {"jsonrpc": "2.0", "id": 4, "method": "tools/call",
         "params": {"name": "save_file", "arguments": {"path": str(path)}}},
    ]
    done = subprocess.run(
        [str(BIN), "mcp"],
        input="".join(json.dumps(call) + "\n" for call in calls),
        capture_output=True,
        text=True,
        timeout=60,
    )
    replies = [json.loads(entry) for entry in done.stdout.splitlines() if entry.strip()]
    for reply in replies:
        assert not reply.get("result", {}).get("isError"), reply
    assert path.is_file(), done.stdout + done.stderr


def check_lists(work, project, run_godot, line):
    vox = work / "path.vox"
    make_list_vox(vox)
    (project / "check_list.gd").write_text(CHECK_LIST)
    bridge = subprocess.Popen(
        [str(BIN), "bridge", "--input", str(vox), "--port", "0"],
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    try:
        session = ""
        assert bridge.stdout is not None
        for _ in range(20):
            entry = bridge.stdout.readline()
            if entry.startswith("FROXEL_SESSION="):
                session = entry.strip().split("=", 1)[1]
                break
        if not session:
            raise SystemExit("no FROXEL_SESSION line for the list document")
        report = line(run_godot("--script", "res://check_list.gd", "--", session).stdout, "LIST=")
        packed_vector2_array = 35
        assert report["schema_error"] == "", report
        assert report["type"] == packed_vector2_array and report["value"] == packed_vector2_array, report
        assert report["set"] == "" and report["wire"] == [[1, 2], [5, 6.5]], report
        assert "at most 4" in report["long"], report
        assert "outside" in report["range"], report
        assert "Expected a list" in report["shape"], report

        def bake(out, values, check=True):
            variants = work / "path_variants.json"
            variants.write_text(json.dumps(values))
            return run_godot(
                "--script", "res://addons/froxel_bridge/bake.gd", "--",
                "--session", session, "--out", out, "--variants", str(variants),
                check=check,
            )

        bake("res://out/path_%d.tscn", [{"path": [[0, 0], [8, 0], [8, 6]]}, {"path": [[2, 2]]}])
        assert (project / "out" / "path_0.tscn").is_file() and (project / "out" / "path_1.tscn").is_file()
        refused = bake("res://out/path_long.tscn", [{"path": [[0, 0]] * 5}], check=False)
        assert refused.returncode != 0, refused.stdout
        assert "at most 4" in refused.stdout + refused.stderr, refused.stdout + refused.stderr
        assert not (project / "out" / "path_long.tscn").exists()
    finally:
        bridge.terminate()
        bridge.wait(timeout=10)


if __name__ == "__main__":
    sys.exit(main())
