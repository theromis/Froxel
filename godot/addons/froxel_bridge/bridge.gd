# SPDX-License-Identifier: MIT
@tool
extends Node3D
## Froxel client. The editor polls in _process; scripts use wait_schema and bake_to.
## Bake creates a plain Node3D scene with embedded resources and no network dependency.

@export_file("*.json") var session_file: String = ""
@export_file("*.tscn") var bake_path: String = "res://baked_froxel.tscn"
@export_storage var schema: Array = []
@export_storage var values: Dictionary = {}

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
	return false

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

func wait_schema(timeout_msec: int = 300000) -> String:
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
	fail("Schema timed out: " + status)
	return fault

func bake_to(path: String, timeout_msec: int = 300000) -> String:
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
	fail("Bake timed out: " + status)
	return fault

func bake() -> void:
	if not is_instance_valid(preview) or dirty or applied != wanted:
		fail("Wait for the latest geometry before baking")
		return
	if not bake_path.begins_with("res://") or bake_path.get_extension() != "tscn":
		fail("Bake path must be res://...tscn")
		return
	var root := preview.duplicate()
	root.name = "BakedFroxel"
	set_owners(root, root)
	var packed := PackedScene.new()
	var error := packed.pack(root)
	if error == OK:
		error = ResourceSaver.save(packed, bake_path, ResourceSaver.FLAG_BUNDLE_RESOURCES)
	root.free()
	if error != OK:
		fail("Bake failed: %s" % error)
	else:
		status = "Baked " + bake_path

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
