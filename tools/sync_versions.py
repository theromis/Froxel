#!/usr/bin/env python3
"""Copy the Froxel version from Cargo.toml into the files the registries read.

    python3 tools/sync_versions.py           # rewrite them
    python3 tools/sync_versions.py --check   # exit 1 when one differs

The files are the Godot plugin.cfg, the Blender manifest and bl_info, the MCP
Registry server.json (version and image tag), the Claude Code plugin.json, and
the Dockerfile's default FROXEL_VERSION. The release workflow runs --check.
"""

import json
import pathlib
import re
import sys

root = pathlib.Path(__file__).resolve().parents[1]


def cargo_version():
    text = (root / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(r'^\[package\][^\[]*?^version\s*=\s*"([^"]+)"', text, re.M | re.S)
    if not match:
        sys.exit("no [package] version in Cargo.toml")
    return match.group(1)


def sub(pattern, replacement):
    def edit(text, version):
        new, count = re.subn(pattern, replacement(version), text, count=1, flags=re.M)
        if count != 1:
            raise ValueError(f"pattern {pattern!r} not found")
        return new

    return edit


def server_json(text, version):
    data = json.loads(text)
    data["version"] = version
    for package in data.get("packages", []):
        if package.get("registryType") == "oci":
            image = package["identifier"].rsplit(":", 1)[0]
            package["identifier"] = f"{image}:{version}"
    return json.dumps(data, indent=2) + "\n"


def plugin_json(text, version):
    data = json.loads(text)
    data["version"] = version
    return json.dumps(data, indent=2) + "\n"


def bl_info(text, version):
    core = re.match(r"(\d+)\.(\d+)\.(\d+)", version)
    tup = "({}, {}, {})".format(*core.groups())
    return sub(r'^(    "version": )\(\d+, \d+, \d+\),$', lambda v: rf"\g<1>{tup},")(text, version)


FILES = {
    "godot/addons/froxel_bridge/plugin.cfg": sub(r'^version=".*"$', lambda v: f'version="{v}"'),
    "blender/froxel_bridge/blender_manifest.toml": sub(r'^version = ".*"$', lambda v: f'version = "{v}"'),
    "blender/froxel_bridge/__init__.py": bl_info,
    "server.json": server_json,
    ".claude-plugin/plugin.json": plugin_json,
    "Dockerfile": sub(r"^ARG FROXEL_VERSION=.*$", lambda v: f"ARG FROXEL_VERSION={v}"),
}


def main():
    check = "--check" in sys.argv[1:]
    version = cargo_version()
    stale = []
    for name, edit in FILES.items():
        path = root / name
        text = path.read_text(encoding="utf-8")
        new = edit(text, version)
        if new == text:
            continue
        stale.append(name)
        if not check:
            path.write_text(new, encoding="utf-8")
    if check and stale:
        print(f"not at {version} (run python3 tools/sync_versions.py): " + ", ".join(stale))
        return 1
    print(f"{version}: " + (", ".join(stale) + " updated" if stale else "all in step"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
