#!/usr/bin/env python3
"""Checks the winget and Scoop manifests from channels.py against their published JSON schemas.

    uv run --with pyyaml --with jsonschema python packaging/windows/check_manifests.py dist/channels

winget's own `winget validate` runs on Windows only; this is the same schema check, so it also
runs on Linux and macOS. The schemas are downloaded from the winget-cli and Scoop repositories.
"""
import json
import sys
import urllib.request
from pathlib import Path

import jsonschema
import yaml

WINGET_SCHEMA = "https://raw.githubusercontent.com/microsoft/winget-cli/master/schemas/JSON/manifests/v{v}/manifest.{kind}.{v}.json"
SCOOP_SCHEMA = "https://raw.githubusercontent.com/ScoopInstaller/Scoop/master/schema.json"


def fetch(url):
    with urllib.request.urlopen(url, timeout=60) as r:
        return json.loads(r.read())


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "dist/channels")
    files = sorted((out / "winget").rglob("*.yaml"))
    if not files:
        sys.exit(f"check_manifests: no winget manifests under {out}/winget")
    schemas = {}
    for f in files:
        # json round trip: YAML reads ReleaseDate as a date, the schema wants its text.
        doc = json.loads(json.dumps(yaml.safe_load(f.read_text()), default=str))
        key = (doc["ManifestType"], doc["ManifestVersion"])
        if key not in schemas:
            schemas[key] = fetch(WINGET_SCHEMA.format(kind=key[0], v=key[1]))
        jsonschema.validate(doc, schemas[key])
        print(f"winget {key[0]}: {f.name} ok")
    scoop = out / "scoop" / "froxel.json"
    jsonschema.validate(json.loads(scoop.read_text()), fetch(SCOOP_SCHEMA))
    print(f"scoop: {scoop.name} ok")


if __name__ == "__main__":
    main()
