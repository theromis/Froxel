# SPDX-License-Identifier: MIT
"""Blender client for a Froxel bridge session."""

bl_info = {
    "name": "Froxel Bridge",
    "author": "Froxel contributors",
    "version": (0, 2, 0),
    "blender": (4, 2, 0),
    "location": "View3D > Sidebar > Froxel",
    "description": "Import a live Froxel model and bake it into the scene",
    "category": "Import-Export",
}

import time

import bpy
from bpy.props import (
    BoolProperty,
    CollectionProperty,
    FloatProperty,
    FloatVectorProperty,
    IntProperty,
    StringProperty,
)

from . import protocol

_suspend = False
_client = None


def _tag(self, context):
    if _suspend or _client is None:
        return
    clamp(self)
    _client.mark()


def clamp(item):
    """Keep a number inside the script's range; Froxel rejects values outside it."""
    global _suspend
    field = {"float": "number", "int": "integer"}.get(item.kind)
    if field is None:
        return
    value = getattr(item, field)
    bounded = min(max(value, item.low), item.high)
    if item.kind == "int":
        bounded = int(bounded)
    if bounded != value:
        _suspend = True
        try:
            setattr(item, field, bounded)
        finally:
            _suspend = False


class FroxelParam(bpy.types.PropertyGroup):
    socket_id: StringProperty()
    kind: StringProperty()
    label: StringProperty()
    number: FloatProperty(update=_tag)
    integer: IntProperty(update=_tag)
    flag: BoolProperty(update=_tag)
    color: FloatVectorProperty(subtype="COLOR", size=3, min=0.0, max=1.0, update=_tag)
    low: FloatProperty()
    high: FloatProperty()


class Client:
    def __init__(self):
        self.sock = None
        self.session = None
        self.frames = protocol.Frames()
        self.out = b""
        self.revision = 0
        self.wanted = -1
        self.dirty = False
        self.due = 0.0
        self.scene = None
        self.status = "Disconnected"
        self.running = False

    def connect(self, path):
        self.close()
        with open(path, encoding="utf-8") as handle:
            self.session = protocol.load_session(handle.read())
        import socket

        sock = socket.socket()
        sock.connect(("127.0.0.1", self.session["port"]))
        sock.setblocking(False)
        self.sock = sock
        self.frames = protocol.Frames()
        self.out = b""
        self.revision = 0
        self.wanted = -1
        self.dirty = False
        self.running = True
        self.scene = bpy.context.scene
        self.status = "Connecting"
        self.send("describe")
        if not bpy.app.timers.is_registered(tick):
            bpy.app.timers.register(tick, persistent=True)

    def close(self):
        self.running = False
        if self.sock is not None:
            self.sock.close()
            self.sock = None
        self.status = "Disconnected"

    def mark(self):
        if self.sock is None:
            return
        self.scene = bpy.context.scene or self.scene
        self.dirty = True
        self.due = time.monotonic() + 0.15

    def send(self, op, extra=None):
        self.revision += 1
        body = {
            "v": 1,
            "id": self.revision,
            "token": self.session["token"],
            "op": op,
        }
        if extra:
            body.update(extra)
        self.out += protocol.encode(body)
        return self.revision

    def poll(self):
        if self.sock is None:
            return
        import socket

        if self.dirty and time.monotonic() >= self.due:
            self.dirty = False
            self.wanted = self.send("evaluate", {"values": snapshot()})
        if self.out:
            try:
                sent = self.sock.send(self.out)
                self.out = self.out[sent:]
            except BlockingIOError:
                pass
        try:
            data = self.sock.recv(65536)
        except BlockingIOError:
            return
        if not data:
            self.close()
            self.status = "Disconnected"
            return
        for message in self.frames.feed(data):
            self.handle(message)

    def handle(self, message):
        if message.get("v") != 1:
            return
        op = message.get("op")
        if op == "error":
            self.status = str(message.get("message"))
            return
        if op == "schema":
            apply_schema(message.get("parameters") or [])
            self.status = "Connected"
            self.mark()
            return
        if op == "result":
            if self.dirty or int(message.get("id", -1)) != self.wanted:
                return
            path = protocol.accept_result(self.session, message.get("path", ""))
            import_result(path)
            self.status = "Applied revision %s" % message["id"]


def snapshot():
    values = {}
    scene = bpy.context.scene or (_client.scene if _client else None)
    if scene is None:
        return values
    for item in scene.froxel_params:
        if item.kind == "float":
            values[item.socket_id] = float(item.number)
        elif item.kind == "int":
            values[item.socket_id] = int(item.integer)
        elif item.kind == "bool":
            values[item.socket_id] = bool(item.flag)
        elif item.kind == "color":
            values[item.socket_id] = [float(item.color[0]), float(item.color[1]), float(item.color[2])]
    return values


def apply_schema(parameters):
    global _suspend
    scene = bpy.context.scene or (_client.scene if _client else None)
    if scene is None:
        raise RuntimeError("No scene")
    _suspend = True
    try:
        scene.froxel_params.clear()
        for spec in parameters:
            item = scene.froxel_params.add()
            item.socket_id = spec["id"]
            item.kind = spec["type"]
            item.label = "%s / %s" % (spec.get("objectName", spec.get("object")), spec["name"])
            value = spec.get("value")
            if spec["type"] in ("float", "int"):
                item.low = float(spec.get("min", value))
                item.high = float(spec.get("max", value))
            if spec["type"] == "float":
                item.number = float(value)
            elif spec["type"] == "int":
                item.integer = int(value)
            elif spec["type"] == "bool":
                item.flag = bool(value)
            elif spec["type"] == "color":
                item.color = (float(value[0]), float(value[1]), float(value[2]))
    finally:
        _suspend = False


def purge_preview():
    doomed = [obj for obj in bpy.data.objects if obj.get("froxel_preview") == 1]
    blocks = []
    for obj in doomed:
        if obj.data is not None:
            blocks.append(obj.data)
        bpy.data.objects.remove(obj, do_unlink=True)
    for block in blocks:
        drop_block(block)


def drop_block(block):
    if block is None or block.users > 0:
        return
    materials = list(getattr(block, "materials", []) or [])
    if isinstance(block, bpy.types.Mesh):
        bpy.data.meshes.remove(block)
    elif isinstance(block, bpy.types.Armature):
        bpy.data.armatures.remove(block)
        return
    else:
        return
    for material in materials:
        if material is None or material.users > 0:
            continue
        images = []
        if material.node_tree:
            for node in material.node_tree.nodes:
                image = getattr(node, "image", None)
                if image is not None:
                    images.append(image)
        bpy.data.materials.remove(material)
        for image in images:
            if image.users == 0:
                bpy.data.images.remove(image)


def import_result(path):
    if not hasattr(bpy.ops.import_scene, "gltf"):
        raise RuntimeError("Enable the glTF 2.0 importer")
    purge_preview()
    before = set(bpy.data.objects)
    window = bpy.context.window
    if window is None and bpy.context.window_manager.windows:
        window = bpy.context.window_manager.windows[0]
    area = None
    region = None
    if window is not None:
        area = next((item for item in window.screen.areas if item.type == "VIEW_3D"), None)
        if area is not None:
            region = next((item for item in area.regions if item.type == "WINDOW"), None)
    if window is not None and area is not None and region is not None:
        with bpy.context.temp_override(window=window, area=area, region=region):
            bpy.ops.import_scene.gltf(filepath=path)
    else:
        bpy.ops.import_scene.gltf(filepath=path)
    for obj in bpy.data.objects:
        if obj not in before:
            obj["froxel_preview"] = 1
    shapes = bone_shapes()
    for obj in shapes:
        if obj.get("froxel_preview") == 1:
            obj["froxel_shape"] = 1
            obj.hide_set(True)
            obj.hide_render = True


def bone_shapes():
    """Objects Blender uses only to draw joints. They are not part of the model."""
    shapes = set()
    for obj in bpy.data.objects:
        if obj.type != "ARMATURE" or obj.pose is None:
            continue
        for bone in obj.pose.bones:
            if bone.custom_shape is not None:
                shapes.add(bone.custom_shape)
    return shapes


def tick():
    if _client is None or not _client.running:
        return None
    try:
        _client.poll()
    except Exception as exc:
        _client.status = str(exc)
    for window in bpy.context.window_manager.windows:
        for area in window.screen.areas:
            if area.type == "VIEW_3D":
                area.tag_redraw()
    return 0.05


class FROXEL_OT_connect(bpy.types.Operator):
    bl_idname = "froxel.connect"
    bl_label = "Connect Froxel"

    def execute(self, context):
        path = bpy.path.abspath(context.scene.froxel_session)
        if not path:
            self.report({"ERROR"}, "Choose a session file")
            return {"CANCELLED"}
        try:
            _client.connect(path)
        except Exception as exc:
            self.report({"ERROR"}, str(exc))
            return {"CANCELLED"}
        return {"FINISHED"}


class FROXEL_OT_disconnect(bpy.types.Operator):
    bl_idname = "froxel.disconnect"
    bl_label = "Stop Froxel Bridge"

    def execute(self, _context):
        _client.close()
        return {"FINISHED"}


class FROXEL_OT_bake(bpy.types.Operator):
    bl_idname = "froxel.bake"
    bl_label = "Bake Froxel into scene"

    def execute(self, context):
        previews = [
            obj
            for obj in bpy.data.objects
            if obj.get("froxel_preview") == 1 and obj.get("froxel_shape") != 1
        ]
        if not previews:
            self.report({"ERROR"}, "Nothing to bake yet")
            return {"CANCELLED"}
        copies = {}
        holder = bpy.data.objects.new("FroxelBake", None)
        context.collection.objects.link(holder)
        for obj in previews:
            copy = obj.copy()
            if obj.data:
                copy.data = obj.data.copy()
            copy["froxel_preview"] = 0
            copies[obj] = copy
            context.collection.objects.link(copy)
        for obj, copy in copies.items():
            world = obj.matrix_world.copy()
            parent = obj.parent
            copy.parent = copies[parent] if parent in copies else holder
            copy.matrix_world = world
            retarget(copy, copies)
        self.report({"INFO"}, "Baked %s" % holder.name)
        return {"FINISHED"}


def retarget(copy, copies):
    """Point a baked copy's modifiers and constraints at the other copies.

    Object.copy() keeps them on the preview, which the next import deletes,
    so a baked mesh would lose its armature.
    """
    owners = list(copy.modifiers) + list(copy.constraints)
    if copy.pose is not None:
        for bone in copy.pose.bones:
            owners.extend(bone.constraints)
    for owner in owners:
        for prop in owner.bl_rna.properties:
            if prop.type != "POINTER" or prop.fixed_type.identifier != "Object":
                continue
            target = getattr(owner, prop.identifier, None)
            if target in copies:
                setattr(owner, prop.identifier, copies[target])


class FROXEL_PT_bridge(bpy.types.Panel):
    bl_space_type = "VIEW_3D"
    bl_region_type = "UI"
    bl_category = "Froxel"
    bl_label = "Froxel Bridge"

    def draw(self, context):
        layout = self.layout
        layout.prop(context.scene, "froxel_session", text="Session")
        row = layout.row(align=True)
        row.operator("froxel.connect", text="Connect / Refresh")
        row.operator("froxel.disconnect", text="Stop")
        layout.label(text=_client.status if _client else "Disconnected")
        for item in context.scene.froxel_params:
            if item.kind == "float":
                layout.prop(item, "number", text=item.label)
            elif item.kind == "int":
                layout.prop(item, "integer", text=item.label)
            elif item.kind == "bool":
                layout.prop(item, "flag", text=item.label)
            elif item.kind == "color":
                layout.prop(item, "color", text=item.label)
        layout.operator("froxel.bake", text="Bake into scene")


CLASSES = (
    FroxelParam,
    FROXEL_OT_connect,
    FROXEL_OT_disconnect,
    FROXEL_OT_bake,
    FROXEL_PT_bridge,
)


def register():
    global _client
    for cls in CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.Scene.froxel_session = StringProperty(name="Session", subtype="FILE_PATH")
    bpy.types.Scene.froxel_params = CollectionProperty(type=FroxelParam)
    _client = Client()


def unregister():
    global _client
    if _client is not None:
        _client.close()
        _client = None
    if bpy.app.timers.is_registered(tick):
        bpy.app.timers.unregister(tick)
    del bpy.types.Scene.froxel_params
    del bpy.types.Scene.froxel_session
    for cls in reversed(CLASSES):
        bpy.utils.unregister_class(cls)
