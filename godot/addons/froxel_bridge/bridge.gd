# SPDX-License-Identifier: MIT
@tool
extends Node3D
## Froxel client. The editor polls in _process; scripts use wait_schema and bake_to.
## Bake creates a plain Node3D scene with embedded resources and no network dependency.
## With the default bake settings that scene is the glTF as imported, in voxel units. The Bake
## settings can scale it to metres, move its origin, keep this node's transform and add a simple
## collision shape; the scene then has an unscaled root with Visual and Collision children.

## The server answers a request within 360 s: 300 s for the scripts of one evaluate, plus a
## minute for a queued job and the export. The client waits a minute longer, so the server's
## own error, such as "bake timed out after 360 s", arrives first.
const WAIT_TIMEOUT_MSEC := 420000

enum BakeOrigin { SOURCE, CENTER, BOTTOM_CENTER }
enum BakeCollision { NONE, BOX, CAPSULE }

@export_file("*.json") var session_file: String = ""
@export_file("*.tscn") var bake_path: String = "res://baked_froxel.tscn"
@export_storage var schema: Array = []
@export_storage var values: Dictionary = {}

@export_group("Bake")
## Metres per voxel. 1 keeps the glTF's units, one unit per voxel. The preview uses it too.
@export_range(0.001, 10.0, 0.001, "or_greater") var bake_voxel_size: float = 1.0:
	set(value):
		bake_voxel_size = value
		place_preview()
## Where the baked scene's origin is. Source keeps the document's origin; Center and Bottom
## Center use the bounds of the geometry. The preview uses it too.
@export var bake_origin: BakeOrigin = BakeOrigin.SOURCE:
	set(value):
		bake_origin = value
		place_preview()
## Bake this node's transform into the scene, so it looks the same placed at the origin of
## another scene as the preview looks here.
@export var bake_apply_transform: bool = false
## A StaticBody3D with one box or one upright capsule around the bounds of the geometry.
@export var bake_collision: BakeCollision = BakeCollision.NONE
@export_group("")

var peer := StreamPeerTCP.new()
var input := PackedByteArray()
var output := PackedByteArray()
var session: Dictionary = {}
var fields: Dictionary = {}
var revision: int = 0
var wanted: int = -1
var applied: int = -1
var dirty: bool = false
var due: int = 0
var connected: bool = false
var preview: Node3D
var status: String = "Disconnected"
var fault: String = ""
var described: bool = false

func connect_bridge() -> void:
	fault = ""
	described = false
	peer.disconnect_from_host()
	connected = false
	input.clear()
	output.clear()
	wanted = -1
	applied = -1
	var parsed = JSON.parse_string(FileAccess.get_file_as_string(session_file))
	if not parsed is Dictionary or parsed.get("host") != "127.0.0.1" or parsed.get("v") != 1:
		fail("Invalid loopback session file")
		return
	session = parsed
	var error := peer.connect_to_host("127.0.0.1", int(session.port))
	if error != OK:
		fail("Connect failed: %s" % error)
	else:
		status = "Connecting"
		due = Time.get_ticks_msec() + 5000

func fail(message: String) -> void:
	status = message
	fault = message
	push_warning("Froxel: " + message)

func send_command(op: String, payload: Dictionary = {}) -> int:
	revision += 1
	payload.merge({"v": 1, "id": revision, "token": session.token, "op": op})
	output.append_array((JSON.stringify(payload) + "\n").to_utf8_buffer())
	return revision

func _process(_delta: float) -> void:
	if Engine.is_editor_hint():
		poll_bridge()

func poll_bridge() -> void:
	peer.poll()
	var state := peer.get_status()
	if state == StreamPeerTCP.STATUS_CONNECTED:
		if not connected:
			connected = true
			status = "Connected"
			send_command("describe")
		if dirty and Time.get_ticks_msec() >= due:
			dirty = false
			wanted = send_command("evaluate", {"values": wire_values()})
		if not output.is_empty():
			var sent := peer.put_partial_data(output)
			if sent[0] != OK:
				fail("Send failed")
				peer.disconnect_from_host()
				return
			output = output.slice(sent[1])
		var count := peer.get_available_bytes()
		if count > 0:
			var received := peer.get_data(count)
			if received[0] != OK:
				return
			input.append_array(received[1])
			while input.find(10) >= 0:
				var end := input.find(10)
				if end > 65536:
					peer.disconnect_from_host()
					return
				var message = JSON.parse_string(input.slice(0, end).get_string_from_utf8())
				input = input.slice(end + 1)
				if message is Dictionary:
					handle(message)
			if input.size() > 65536:
				peer.disconnect_from_host()
	elif connected:
		connected = false
		fail("Disconnected. Click Connect / Refresh to reconnect.")
	elif state == StreamPeerTCP.STATUS_CONNECTING and Time.get_ticks_msec() > due:
		peer.disconnect_from_host()
		fail("Connection timed out")

func wire_values() -> Dictionary:
	var result := {}
	for key in values:
		var value = values[key]
		if value is Color:
			value = [value.r, value.g, value.b]
		elif value is PackedFloat32Array:
			value = Array(value)
		elif value is PackedVector2Array:
			value = Array(value).map(func(point: Vector2): return [point.x, point.y])
		elif value is PackedVector3Array:
			value = Array(value).map(func(point: Vector3): return [point.x, point.y, point.z])
		result[key] = value
	return result

func handle(message: Dictionary) -> void:
	if message.get("v") != 1:
		return
	if message.get("op") == "error":
		fail(str(message.get("message")))
	elif message.get("op") == "schema":
		apply_schema(message.parameters)
	elif message.get("op") == "result":
		if dirty or int(message.id) != wanted:
			return
		var path := str(message.path)
		if not accept_result(path):
			fail("Result path outside session directory")
			return
		var node := load_preview_scene(path)
		if node == null:
			return
		if is_instance_valid(preview):
			remove_child(preview)
			preview.queue_free()
		prepare_materials(node)
		preview = node
		add_child(preview)
		place_preview()
		applied = wanted
		fault = ""
		status = "Applied revision %s" % applied

func accept_result(path: String) -> bool:
	var root := str(session.root).simplify_path().trim_suffix("/")
	var file := path.simplify_path()
	return file.get_base_dir() == root and file.get_extension() == "glb" and not file.get_file().contains("..")

func apply_schema(incoming: Array) -> void:
	schema = incoming
	var previous := values.duplicate()
	values.clear()
	fields.clear()
	var name_count := {}
	var seen_object := {}
	for spec in schema:
		var key := "%s" % spec.object
		if seen_object.has(key):
			continue
		seen_object[key] = true
		var object_name := str(spec.objectName)
		name_count[object_name] = int(name_count.get(object_name, 0)) + 1
	for spec in schema:
		var value = spec.value
		match str(spec.type):
			"color":
				value = Color(float(value[0]), float(value[1]), float(value[2]))
			"int":
				value = int(value)
			"bool":
				value = bool(value)
			"float":
				value = float(value)
			"floats", "vec2s", "vec3s":
				value = to_list(value, str(spec.type))
		var id := str(spec.id)
		values[id] = previous[id] if still_valid(previous.get(id), spec) else value
		var group := str(spec.objectName).replace("/", " ")
		if group == "":
			group = "Object %s" % spec.object
		if int(name_count.get(str(spec.objectName), 0)) > 1:
			group = "%s %s" % [group, spec.object]
		var label := str(spec.name).replace("/", " ")
		var prop := "%s/%s" % [group, label]
		if fields.has(prop):
			prop = "%s/%s" % [group, id.replace("/", " ")]
		fields[prop] = id
	notify_property_list_changed()
	queue_evaluation()
	described = true
	fault = ""

## A value kept from an earlier schema stays only while it has the same type and
## range; otherwise every evaluation would be rejected until it is edited.
func still_valid(value: Variant, spec: Dictionary) -> bool:
	match str(spec.type):
		"bool":
			return value is bool
		"color":
			return value is Color
		"int":
			return value is int and value >= int(spec.min) and value <= int(spec.max)
		"float":
			return value is float and value >= float(spec.min) and value <= float(spec.max)
		"floats", "vec2s", "vec3s":
			return typeof(value) == LIST_TYPES[str(spec.type)] and list_error(value, spec) == ""
	return false

const LIST_TYPES := {
	"floats": TYPE_PACKED_FLOAT32_ARRAY,
	"vec2s": TYPE_PACKED_VECTOR2_ARRAY,
	"vec3s": TYPE_PACKED_VECTOR3_ARRAY,
}

## A list parameter as its packed array: numbers for floats, [x, y] or Vector2 for vec2s,
## [x, y, z] or Vector3 for vec3s. Null when an entry has the wrong shape.
func to_list(value: Variant, kind: String) -> Variant:
	if typeof(value) == LIST_TYPES[kind]:
		return value
	if not (value is Array or value is PackedFloat32Array or value is PackedFloat64Array \
			or value is PackedVector2Array or value is PackedVector3Array):
		return null
	var out: Variant
	match kind:
		"floats":
			out = PackedFloat32Array()
		"vec2s":
			out = PackedVector2Array()
		_:
			out = PackedVector3Array()
	for entry in value:
		match kind:
			"floats":
				if not (entry is int or entry is float):
					return null
				out.append(float(entry))
			"vec2s":
				if entry is Vector2:
					out.append(entry)
				elif entry is Array and entry.size() == 2:
					out.append(Vector2(float(entry[0]), float(entry[1])))
				else:
					return null
			_:
				if entry is Vector3:
					out.append(entry)
				elif entry is Array and entry.size() == 3:
					out.append(Vector3(float(entry[0]), float(entry[1]), float(entry[2])))
				else:
					return null
	return out

## "" when a packed list fits the parameter's length and range, else the reason.
func list_error(value: Variant, spec: Dictionary) -> String:
	if value.size() > int(spec.maxLen):
		return "has %d entries; at most %d are allowed" % [value.size(), int(spec.maxLen)]
	var low := float(spec.min)
	var high := float(spec.max)
	for index in value.size():
		var entry = value[index]
		var parts: Array = [entry] if entry is float else ([entry.x, entry.y] if entry is Vector2 else [entry.x, entry.y, entry.z])
		for part in parts:
			if part < low or part > high:
				return "entry %d is outside %s..=%s" % [index, spec.min, spec.max]
	return ""

func queue_evaluation() -> void:
	dirty = true
	due = Time.get_ticks_msec() + 150

func _get_property_list() -> Array[Dictionary]:
	var properties: Array[Dictionary] = [{
		"name": "bridge_status",
		"type": TYPE_STRING,
		"usage": PROPERTY_USAGE_EDITOR | PROPERTY_USAGE_READ_ONLY,
	}]
	var types := {"bool": TYPE_BOOL, "int": TYPE_INT, "float": TYPE_FLOAT, "color": TYPE_COLOR}
	types.merge(LIST_TYPES)
	var opened := {}
	for prop in fields:
		var spec := spec_by_id(str(fields[prop]))
		if spec.is_empty() or not types.has(str(spec.type)):
			continue
		var group := str(prop).get_slice("/", 0)
		if not opened.has(group):
			opened[group] = true
			# The prefix strips "group/" from the names below, so they do not
			# nest a second section of the same name.
			properties.append({"name": group, "type": TYPE_NIL, "usage": PROPERTY_USAGE_GROUP, "hint_string": group + "/"})
		var entry := {"name": prop, "type": types[str(spec.type)], "usage": PROPERTY_USAGE_EDITOR}
		if spec.type == "int" or spec.type == "float":
			entry["hint"] = PROPERTY_HINT_RANGE
			var step := "1" if spec.type == "int" else "0.01"
			entry["hint_string"] = "%s,%s,%s" % [spec.min, spec.max, step]
		properties.append(entry)
	return properties

func _get(property: StringName) -> Variant:
	if property == "bridge_status":
		return status
	if fields.has(str(property)):
		return values.get(fields[str(property)])
	return null

func _set(property: StringName, value: Variant) -> bool:
	if not fields.has(str(property)):
		return false
	values[fields[str(property)]] = value
	queue_evaluation()
	return true

func spec_by_id(id: String) -> Dictionary:
	for spec in schema:
		if str(spec.id) == id:
			return spec
	return {}

func parameter_spec(parameter_name: String) -> Dictionary:
	var by_name: Dictionary = {}
	var named := 0
	for spec in schema:
		if str(spec.id) == parameter_name:
			return spec
		var object_name := "%s/%s" % [spec.objectName, spec.name]
		if str(spec.name) == parameter_name or object_name == parameter_name:
			by_name = spec
			named += 1
	if named == 1:
		return by_name
	return {}

func property_for(id: String) -> String:
	for prop in fields:
		if str(fields[prop]) == id:
			return str(prop)
	return ""

func set_parameter(parameter_name: String, value: Variant) -> String:
	var spec := parameter_spec(parameter_name)
	if spec.is_empty():
		fail("Unknown parameter: " + parameter_name)
		return fault
	var typed: Variant = value
	match str(spec.type):
		"bool":
			if not (value is bool):
				fail("Expected bool for " + parameter_name)
				return fault
		"int":
			if not ((value is int) or (value is float and value == int(value))):
				fail("Expected int for " + parameter_name)
				return fault
			typed = int(value)
		"float":
			if not ((value is int) or (value is float)):
				fail("Expected float for " + parameter_name)
				return fault
			typed = float(value)
		"color":
			if value is Array and (value.size() == 3 or value.size() == 4):
				typed = Color(value[0], value[1], value[2])
			elif not (value is Color):
				fail("Expected color for " + parameter_name)
				return fault
		"floats", "vec2s", "vec3s":
			typed = to_list(value, str(spec.type))
			if typed == null:
				fail("Expected a list of %s for %s" % [{"floats": "numbers", "vec2s": "[x, y] points", "vec3s": "[x, y, z] points"}[str(spec.type)], parameter_name])
				return fault
			var reason := list_error(typed, spec)
			if reason != "":
				fail("%s %s" % [parameter_name, reason])
				return fault
		_:
			fail("Unsupported parameter type for " + parameter_name)
			return fault
	var prop := property_for(str(spec.id))
	if prop == "":
		fail("Cannot set " + parameter_name)
		return fault
	set(prop, typed)
	if values.get(str(spec.id)) != typed:
		fail("Cannot set " + parameter_name)
		return fault
	fault = ""
	return ""

func wait_schema(timeout_msec: int = WAIT_TIMEOUT_MSEC) -> String:
	if fault != "":
		return fault
	if get_tree() == null:
		fail("Bridge node must be in the tree")
		return fault
	var limit := Time.get_ticks_msec() + timeout_msec
	while Time.get_ticks_msec() < limit:
		if not Engine.is_editor_hint():
			poll_bridge()
		if fault != "":
			return fault
		if described:
			return ""
		await get_tree().process_frame
	fail("Schema timed out after %s s: %s" % [timeout_msec / 1000.0, status])
	return fault

func bake_to(path: String, timeout_msec: int = WAIT_TIMEOUT_MSEC) -> String:
	if fault != "":
		return fault
	if get_tree() == null:
		fail("Bridge node must be in the tree")
		return fault
	if not path.begins_with("res://") or path.get_extension() != "tscn":
		fail("Bake path must be res://...tscn")
		return fault
	var directory := path.get_base_dir()
	if directory != "res://" and not DirAccess.dir_exists_absolute(directory):
		var created := DirAccess.make_dir_recursive_absolute(directory)
		if created != OK:
			fail("Cannot create %s: %s" % [directory, created])
			return fault
	bake_path = path
	var limit := Time.get_ticks_msec() + timeout_msec
	while Time.get_ticks_msec() < limit:
		if not Engine.is_editor_hint():
			poll_bridge()
		if fault != "":
			return fault
		if applied == wanted and applied >= 0 and not dirty and is_instance_valid(preview):
			bake()
			return fault
		await get_tree().process_frame
	fail("Bake timed out after %s s: %s" % [timeout_msec / 1000.0, status])
	return fault

func bake() -> void:
	if not is_instance_valid(preview) or dirty or applied != wanted:
		fail("Wait for the latest geometry before baking")
		return
	if not bake_path.begins_with("res://") or bake_path.get_extension() != "tscn":
		fail("Bake path must be res://...tscn")
		return
	var root := build_bake()
	if root == null:
		return
	set_owners(root, root)
	var packed := PackedScene.new()
	var error := packed.pack(root)
	root.free()
	# Written beside the target and moved over it only once complete, so a failed bake leaves
	# the previous scene in place.
	var staging := bake_path.get_basename() + ".baking.tscn"
	if error == OK:
		error = ResourceSaver.save(packed, staging, ResourceSaver.FLAG_BUNDLE_RESOURCES)
	if error == OK:
		error = DirAccess.rename_absolute(staging, bake_path)
	if error != OK:
		if FileAccess.file_exists(staging):
			DirAccess.remove_absolute(staging)
		fail("Bake failed: %s" % error)
	else:
		packed.take_over_path(bake_path)
		status = "Baked " + bake_path

## True when every bake setting has its default, which bakes the import as it is.
func bake_is_raw() -> bool:
	return is_equal_approx(bake_voxel_size, 1.0) and bake_origin == BakeOrigin.SOURCE \
		and not bake_apply_transform and bake_collision == BakeCollision.NONE

## The scene root to save, or null after fail().
func build_bake() -> Node3D:
	if bake_voxel_size <= 0.0:
		fail("Bake voxel size must be above 0")
		return null
	if bake_is_raw():
		var raw := preview.duplicate() as Node3D
		raw.name = "BakedFroxel"
		return raw
	var bounds := voxel_bounds(preview)
	if bounds.size == Vector3.ZERO:
		fail("Nothing to bake: the geometry is empty")
		return null
	var root := Node3D.new()
	root.name = "BakedFroxel"
	var visual := preview.duplicate() as Node3D
	visual.name = "Visual"
	visual.transform = bake_transform(bounds)
	if bake_apply_transform:
		visual.transform = transform * visual.transform
	root.add_child(visual)
	if bake_collision != BakeCollision.NONE:
		var placed := visual.transform * bounds
		var body := StaticBody3D.new()
		body.name = "Collision"
		var shape := CollisionShape3D.new()
		shape.name = "Shape"
		shape.position = placed.get_center()
		if bake_collision == BakeCollision.BOX:
			var box := BoxShape3D.new()
			box.size = placed.size
			shape.shape = box
		else:
			var capsule := CapsuleShape3D.new()
			capsule.radius = maxf(placed.size.x, placed.size.z) * 0.5
			capsule.height = maxf(placed.size.y, capsule.radius * 2.0)
			shape.shape = capsule
		body.add_child(shape)
		root.add_child(body)
	return root

## Voxel size and origin, from the glTF's own space. This node's transform is not included.
func bake_transform(bounds: AABB) -> Transform3D:
	var origin := Vector3.ZERO
	match bake_origin:
		BakeOrigin.CENTER:
			origin = bounds.get_center()
		BakeOrigin.BOTTOM_CENTER:
			origin = Vector3(bounds.get_center().x, bounds.position.y, bounds.get_center().z)
	return Transform3D(Basis.from_scale(Vector3.ONE * bake_voxel_size), -origin * bake_voxel_size)

## Shows the preview at the bake's voxel size and origin.
func place_preview() -> void:
	if not is_instance_valid(preview):
		return
	if bake_voxel_size <= 0.0:
		return
	preview.transform = bake_transform(voxel_bounds(preview))

## The bounds of every mesh under node, in node's own space (its transform left out).
func voxel_bounds(node: Node3D) -> AABB:
	var found := {"bounds": AABB(), "any": false}
	if node is MeshInstance3D and (node as MeshInstance3D).mesh != null:
		found["bounds"] = (node as MeshInstance3D).mesh.get_aabb()
		found["any"] = true
	collect_bounds(node, Transform3D.IDENTITY, found)
	return found["bounds"]

func collect_bounds(node: Node, to_root: Transform3D, found: Dictionary) -> void:
	for child in node.get_children():
		if not child is Node3D:
			continue
		var placed: Transform3D = to_root * (child as Node3D).transform
		if child is MeshInstance3D and (child as MeshInstance3D).mesh != null:
			var box: AABB = placed * (child as MeshInstance3D).mesh.get_aabb()
			found["bounds"] = box if not found["any"] else (found["bounds"] as AABB).merge(box)
			found["any"] = true
		collect_bounds(child, placed, found)

func set_owners(node: Node, root: Node) -> void:
	for child in node.get_children():
		child.owner = root
		set_owners(child, root)

func _exit_tree() -> void:
	peer.disconnect_from_host()

func prepare_materials(node: Node) -> void:
	if node is MeshInstance3D and node.mesh != null:
		for index in range(node.mesh.get_surface_count()):
			var arrays: Array = node.mesh.surface_get_arrays(index)
			if arrays[Mesh.ARRAY_COLOR] != null and not arrays[Mesh.ARRAY_COLOR].is_empty():
				var material = node.get_active_material(index)
				if material is BaseMaterial3D:
					material = material.duplicate()
					material.vertex_color_use_as_albedo = true
					node.set_surface_override_material(index, material)
	for child in node.get_children():
		prepare_materials(child)

func load_preview_scene(path: String) -> Node3D:
	var document := GLTFDocument.new()
	var gltf_state := GLTFState.new()
	# The default extracts textures as files beside the glb, which in the editor
	# fails for a glb outside res:// and leaves the preview and bakes untextured.
	gltf_state.handle_binary_image = GLTFState.HANDLE_BINARY_EMBED_AS_UNCOMPRESSED
	var converter: GLTFDocumentExtension
	if Engine.is_editor_hint():
		converter = GLTFDocumentExtensionConvertImporterMesh.new()
		GLTFDocument.register_gltf_document_extension(converter, true)
	var error := document.append_from_file(path, gltf_state)
	var node: Node3D
	if error == OK:
		node = document.generate_scene(gltf_state)
	if converter != null:
		GLTFDocument.unregister_gltf_document_extension(converter)
	if node == null:
		fail("Cannot load renderable GLB scene")
	return node
