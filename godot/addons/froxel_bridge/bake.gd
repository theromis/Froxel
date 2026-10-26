# SPDX-License-Identifier: MIT
extends SceneTree
## Headless bake. One .tscn per variants.json object. Prints each path and exits non-zero on failure.
## --voxel-size, --origin and --collision set the bridge node's bake settings; without them the
## scenes are the raw import, as before.

const ORIGINS := {"source": 0, "center": 1, "bottom_center": 2}
const COLLISIONS := {"none": 0, "box": 1, "capsule": 2}

func _initialize() -> void:
	call_deferred("run")

func option(name: String) -> String:
	var args := OS.get_cmdline_user_args()
	var index := args.find(name)
	if index < 0 or index + 1 >= args.size():
		return ""
	return args[index + 1]

func fail(message: String) -> void:
	push_error(message)
	quit(1)

func run() -> void:
	var session := option("--session")
	var out := option("--out")
	var variants_path := option("--variants")
	if session == "" or out == "" or variants_path == "":
		fail("Usage: --session <session.json> --out <res://name_%d.tscn> --variants <variants.json>")
		return
	if not out.begins_with("res://") or out.get_extension() != "tscn":
		fail("Bake path must be res://...tscn")
		return
	if not FileAccess.file_exists(variants_path):
		fail("Cannot read " + variants_path)
		return
	var parsed = JSON.parse_string(FileAccess.get_file_as_string(variants_path))
	if not parsed is Array or parsed.is_empty():
		fail("variants.json must be a non-empty list of parameter objects")
		return
	if parsed.size() > 1 and not out.contains("%d"):
		fail("--out needs %d when baking more than one variant")
		return
	var voxel_size := 1.0
	var size_text := option("--voxel-size")
	if size_text != "":
		if not size_text.is_valid_float() or size_text.to_float() <= 0.0:
			fail("--voxel-size must be a number above 0, in metres per voxel")
			return
		voxel_size = size_text.to_float()
	var origin := option("--origin")
	if origin == "":
		origin = "source"
	if not ORIGINS.has(origin):
		fail("--origin must be source, center or bottom_center")
		return
	var collision := option("--collision")
	if collision == "":
		collision = "none"
	if not COLLISIONS.has(collision):
		fail("--collision must be none, box or capsule")
		return
	var bridge: Node3D = load("res://addons/froxel_bridge/bridge.gd").new()
	bridge.set("bake_voxel_size", voxel_size)
	bridge.set("bake_origin", ORIGINS[origin])
	bridge.set("bake_collision", COLLISIONS[collision])
	root.add_child(bridge)
	bridge.session_file = session
	bridge.connect_bridge()
	var error: String = await bridge.wait_schema()
	if error != "":
		fail(error)
		return
	for index in parsed.size():
		var item = parsed[index]
		if not item is Dictionary:
			fail("Variant %d is not an object" % index)
			return
		for key in item:
			error = bridge.set_parameter(str(key), item[key])
			if error != "":
				fail(error)
				return
		var path := out.replace("%d", str(index))
		error = await bridge.bake_to(path)
		if error != "":
			fail(error)
			return
		print(path)
	quit(0)
