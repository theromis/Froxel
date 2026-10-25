# SPDX-License-Identifier: MIT
@tool
extends EditorInspectorPlugin

func _can_handle(object: Object) -> bool:
	return object.get_script() == preload("bridge.gd")

func _parse_begin(object: Object) -> void:
	var box := VBoxContainer.new()
	for action in ["connect_bridge", "bake"]:
		var button := Button.new()
		button.text = "Connect / Refresh" if action == "connect_bridge" else "Bake standalone scene"
		button.pressed.connect(Callable(object, action))
		box.add_child(button)
	add_custom_control(box)
