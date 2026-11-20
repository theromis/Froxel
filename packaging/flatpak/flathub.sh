#!/usr/bin/env bash
# Writes what Flathub builds: the manifest for a release tag and its
# cargo-sources.json (every crate in Cargo.lock, so the build runs offline).
#
#   packaging/flatpak/flathub.sh <tag> <out-dir>   # e.g. v0.3.4 ../flathub-froxel
#   packaging/flatpak/flathub.sh --local           # cargo-sources.json here, for a local build
#
# Needs python3 with aiohttp and tomlkit (or toml), and git. The generator is
# flatpak-cargo-generator.py from flatpak/flatpak-builder-tools at a pinned commit.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
repo_root=$(cd "$here/../.." && pwd)
url=${REPOSITORY_URL:-https://github.com/theromis/Froxel.git}
tools_commit=${FLATPAK_BUILDER_TOOLS_COMMIT:-9d86d336a39b9af38b86f114b6ce5820fe260f91}
generator_url="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/$tools_commit/cargo/flatpak-cargo-generator.py"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl -fsSL "$generator_url" -o "$work/flatpak-cargo-generator.py"

if [ "${1:-}" = "--local" ]; then
  python3 -I "$work/flatpak-cargo-generator.py" "$repo_root/Cargo.lock" -o "$here/cargo-sources.json"
  echo "wrote $here/cargo-sources.json"
  exit 0
fi

tag=${1:?tag}
out=${2:?out-dir}
mkdir -p "$out"
commit=$(git -C "$repo_root" rev-list -n1 "$tag")
git -C "$repo_root" show "$commit:Cargo.lock" >"$work/Cargo.lock"
python3 -I "$work/flatpak-cargo-generator.py" "$work/Cargo.lock" -o "$out/cargo-sources.json"

# The same manifest with the checkout replaced by the tagged commit.
python3 -I - "$here/com.lonje.Froxel.yml" "$out/com.lonje.Froxel.yml" "$url" "$tag" "$commit" <<'PY'
import sys
src, dst, url, tag, commit = sys.argv[1:]
lines = open(src).read().splitlines()
out, i = [], 0
while lines and lines[0].startswith("#"):
    lines.pop(0)
while i < len(lines):
    line = lines[i]
    if line.strip() == "- type: dir":
        indent = line[: len(line) - len(line.lstrip())]
        out += [f"{indent}- type: git", f"{indent}  url: {url}", f"{indent}  tag: {tag}", f"{indent}  commit: {commit}"]
        i += 1
        while i < len(lines) and lines[i].startswith(indent + "  "):
            i += 1
        continue
    out.append(line)
    i += 1
open(dst, "w").write("\n".join(out) + "\n")
PY
echo "wrote $out/com.lonje.Froxel.yml and $out/cargo-sources.json for $tag ($commit)"
