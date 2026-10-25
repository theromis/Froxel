# SPDX-License-Identifier: MIT
@tool
extends EditorPlugin

var inspector: EditorInspectorPlugin

func _enter_tree() -> void:
	add_custom_type("FroxelBridge", "Node3D", preload("bridge.gd"), null)
	inspector = preload("inspector.gd").new()
	add_inspector_plugin(inspector)

func _exit_tree() -> void:
	remove_inspector_plugin(inspector)
	remove_custom_type("FroxelBridge")
