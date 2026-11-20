#!/usr/bin/env python3
"""Writes the winget, Scoop, and Chocolatey manifests for one release from its SHA256SUMS.txt.

    python3 packaging/windows/channels.py 0.3.3 SHA256SUMS.txt --out dist/channels

writes

    dist/channels/winget/manifests/l/Lonje/Froxel/0.3.3/Lonje.Froxel*.yaml
    dist/channels/scoop/froxel.json
    dist/channels/chocolatey/froxel.nuspec and tools/chocolateyinstall.ps1

Every channel installs the portable zip of the GitHub release (`froxel-<version>-windows-<arch>.zip`,
which holds `froxel-<version>-windows-<arch>/froxel.exe`), so nothing is built here.
`--release-date` (YYYY-MM-DD) adds winget's ReleaseDate. The Scoop manifest is also kept at
bucket/froxel.json; .github/workflows/windows-repos.yml copies the new one there.
"""
import argparse
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = "theromis/Froxel"
WINGET_ID = "Lonje.Froxel"
WINGET_SCHEMA = "1.10.0"
ARCHES = ("x64", "x86", "arm64")
SHORT = "Voxel editor with a path tracer, rigging, glTF export, and an MCP server for agents."
DESCRIPTION = (
    "Froxel is a voxel editor for MagicaVoxel .vox models: brushes and sculpt tools, bones and "
    "animation clips, a GPU path tracer, glTF export for game engines, and an MCP server "
    "(froxel mcp) that lets an agent build and check models. It also bridges to Godot and Blender."
)
TAGS = ["voxel", "vox", "magicavoxel", "3d", "editor", "path-tracer", "gltf", "mcp"]


def sums(path, version):
    """The SHA-256 of each Windows zip, by architecture."""
    found = {}
    for line in Path(path).read_text().splitlines():
        parts = line.split()
        if len(parts) != 2:
            continue
        digest, name = parts[0].lower(), parts[1].lstrip("*")
        for arch in ARCHES:
            if name == f"froxel-{version}-windows-{arch}.zip":
                if not re.fullmatch(r"[0-9a-f]{64}", digest):
                    sys.exit(f"channels: {name} has no SHA-256 in {path}")
                found[arch] = digest
    missing = [a for a in ARCHES if a not in found]
    if missing:
        sys.exit(f"channels: {path} has no Windows zip for {', '.join(missing)} of {version}")
    return found


def url(version, arch):
    return f"https://github.com/{REPO}/releases/download/v{version}/froxel-{version}-windows-{arch}.zip"


def q(text):
    """A YAML scalar for free text: a JSON string is a valid YAML double-quoted one."""
    return json.dumps(text)


def yaml_list(items, indent=""):
    return "".join(f"{indent}- {item}\n" for item in items)


def winget(version, digests, out, release_date):
    publisher, name = WINGET_ID.split(".", 1)
    folder = out / "winget" / "manifests" / publisher[0].lower() / publisher / name / version
    folder.mkdir(parents=True, exist_ok=True)
    head = (
        "# Created by packaging/windows/channels.py in https://github.com/" + REPO + "\n"
        "# yaml-language-server: $schema=https://aka.ms/winget-manifest.{kind}." + WINGET_SCHEMA + ".schema.json\n\n"
        f"PackageIdentifier: {WINGET_ID}\n"
        f"PackageVersion: {version}\n"
    )
    tail = f"ManifestType: {{kind}}\nManifestVersion: {WINGET_SCHEMA}\n"
    (folder / f"{WINGET_ID}.yaml").write_text(
        head.format(kind="version")
        + "DefaultLocale: en-US\n"
        + tail.format(kind="version")
    )
    installers = ""
    for arch in ARCHES:
        installers += (
            f"- Architecture: {arch}\n"
            f"  InstallerUrl: {url(version, arch)}\n"
            f"  InstallerSha256: {digests[arch].upper()}\n"
            "  NestedInstallerFiles:\n"
            f"  - RelativeFilePath: froxel-{version}-windows-{arch}\\froxel.exe\n"
            "    PortableCommandAlias: froxel\n"
        )
    (folder / f"{WINGET_ID}.installer.yaml").write_text(
        head.format(kind="installer")
        + "InstallerType: zip\n"
        + "NestedInstallerType: portable\n"
        + "Commands:\n"
        + yaml_list(["froxel"])
        + "FileExtensions:\n"
        + yaml_list(["vox"])
        + (f"ReleaseDate: {release_date}\n" if release_date else "")
        + "Installers:\n"
        + installers
        + tail.format(kind="installer")
    )
    (folder / f"{WINGET_ID}.locale.en-US.yaml").write_text(
        head.format(kind="defaultLocale")
        + "PackageLocale: en-US\n"
        + "Publisher: Roman Vasilyev\n"
        + f"PublisherUrl: https://github.com/{REPO.split('/')[0]}\n"
        + f"PublisherSupportUrl: https://github.com/{REPO}/issues\n"
        + "Author: Roman Vasilyev\n"
        + "PackageName: Froxel\n"
        + f"PackageUrl: https://github.com/{REPO}\n"
        + "License: MIT\n"
        + f"LicenseUrl: https://github.com/{REPO}/blob/main/LICENSE\n"
        + f"Copyright: {q('Copyright (c) 2026 Roman Vasilyev')}\n"
        + f"ShortDescription: {q(SHORT)}\n"
        + f"Description: {q(DESCRIPTION)}\n"
        + "Moniker: froxel\n"
        + "Tags:\n"
        + yaml_list(TAGS)
        + f"ReleaseNotesUrl: https://github.com/{REPO}/releases/tag/v{version}\n"
        + tail.format(kind="defaultLocale")
    )
    return folder


SCOOP_ARCH = {"x64": "64bit", "x86": "32bit", "arm64": "arm64"}


def scoop(version, digests, out):
    def entry(arch, ver, digest=None):
        e = {"url": url(ver, arch)}
        if digest:
            e["hash"] = digest
        e["extract_dir"] = f"froxel-{ver}-windows-{arch}"
        return e

    manifest = {
        "version": version,
        "description": SHORT,
        "homepage": f"https://github.com/{REPO}",
        "license": "MIT",
        "notes": "Windows builds are not code-signed yet; SmartScreen may ask on the first launch.",
        "architecture": {SCOOP_ARCH[a]: entry(a, version, digests[a]) for a in ARCHES},
        "bin": "froxel.exe",
        "shortcuts": [["froxel.exe", "Froxel"]],
        "checkver": {"github": f"https://github.com/{REPO}"},
        "autoupdate": {
            "architecture": {SCOOP_ARCH[a]: entry(a, "$version") for a in ARCHES},
            "hash": {"url": "$baseurl/SHA256SUMS.txt"},
        },
    }
    (out / "scoop").mkdir(parents=True, exist_ok=True)
    path = out / "scoop" / "froxel.json"
    path.write_text(json.dumps(manifest, indent=4) + "\n")
    return path


def choco_version(version):
    """NuGet's prerelease tags take no dots: 0.4.0-rc.1 is 0.4.0-rc01. The number is
    padded to two digits (build_number.sh allows 1 to 98) so rc10 sorts after rc09."""
    core, _, pre = version.partition("-")
    if not pre:
        return version
    m = re.fullmatch(r"([A-Za-z.]*?)\.?([0-9]+)", pre)
    if not m:
        sys.exit(f"channels: Chocolatey needs a prerelease ending in a number, not {version}")
    label = re.sub(r"[^A-Za-z]", "", m.group(1)) or "pre"
    return f"{core}-{label}{int(m.group(2)):02d}"


def chocolatey(version, digests, out):
    fields = {"VERSION": choco_version(version), "RELEASE_TAG": f"v{version}"}
    for arch in ARCHES:
        fields[f"URL_{arch.upper()}"] = url(version, arch)
        fields[f"SHA256_{arch.upper()}"] = digests[arch]
    folder = out / "chocolatey"
    (folder / "tools").mkdir(parents=True, exist_ok=True)
    for rel in ("froxel.nuspec", "tools/chocolateyinstall.ps1"):
        text = (HERE / "chocolatey" / rel).read_text()
        text = re.sub(r"@([A-Z0-9_]+)@", lambda m: fields[m.group(1)], text)
        (folder / rel).write_text(text)
    return folder


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("version", help="the release version, e.g. 0.3.3 (no v)")
    ap.add_argument("sums", help="the release's SHA256SUMS.txt")
    ap.add_argument("--out", default="dist/channels")
    ap.add_argument("--release-date", default="")
    args = ap.parse_args()
    version = args.version.removeprefix("v")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?", version):
        sys.exit(f"channels: '{version}' is not a version like 1.2.3 or 1.2.3-rc.1")
    if args.release_date and not re.fullmatch(r"\d{4}-\d{2}-\d{2}", args.release_date):
        sys.exit("channels: --release-date is YYYY-MM-DD")
    digests = sums(args.sums, version)
    out = Path(args.out)
    print(winget(version, digests, out, args.release_date))
    print(scoop(version, digests, out))
    print(chocolatey(version, digests, out))


if __name__ == "__main__":
    main()
