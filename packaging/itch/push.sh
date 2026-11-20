#!/usr/bin/env bash
# Pushes a Froxel release to itch.io with butler, one channel per platform.
#
#   packaging/itch/push.sh <release-dir> <version>
#
# <release-dir> holds the GitHub Release files (froxel-<version>-*). Run from
# the repository root. Each build is unpacked into its own folder with the
# channel's .itch.toml at the root, and pushed with --userversion <version>:
#
#   osx-universal   froxel-<version>-macos-universal.zip (Froxel.app)
#   windows-x64     froxel-<version>-windows-x64.zip
#   windows-x86     froxel-<version>-windows-x86.zip
#   windows-arm64   froxel-<version>-windows-arm64.zip
#   linux-x86_64    froxel-<version>-linux-x86_64.tar.gz
#   linux-aarch64   froxel-<version>-linux-aarch64.tar.gz
#   android         froxel-<version>-android-arm64.apk
#
# A missing file fails the run, so a channel is never left a version behind
# without notice. Environment:
#   ITCH_PROJECT  user/game on itch.io (default theromis/froxel)
#   BUTLER        the butler binary (default butler)
#   DRY_RUN=1     unpack, check, and print the pushes without running them
#   VALIDATE=1    run `butler validate` on every folder (needs no login)
# butler reads BUTLER_API_KEY itself.
set -euo pipefail

release=${1:?release dir}
version=${2:?version}
project=${ITCH_PROJECT:-theromis/froxel}
butler=${BUTLER:-butler}
here=$(cd "$(dirname "$0")" && pwd)

case "$version" in
*-*)
  # Prereleases go to the Steam beta branch only; the itch page lists releases.
  echo "itch: $version is a prerelease, nothing pushed"
  exit 0
  ;;
esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT


# ditto zips may carry AppleDouble files (._*, __MACOSX/) for extended
# attributes; they are not part of the build.
unzip_clean() {
  unzip -q "$1" -d "$2"
  rm -rf "$2/__MACOSX"
  find "$2" -name '._*' -exec rm -rf {} +
}

# The archive's single top folder (froxel-<version>-<platform>/), unpacked.
unpack_top() {
  local archive=$1 dir=$2 top
  mkdir -p "$dir.x"
  case "$archive" in
  *.zip) unzip_clean "$archive" "$dir.x" ;;
  *.tar.gz) tar -C "$dir.x" -xzf "$archive" ;;
  esac
  top=$(find "$dir.x" -mindepth 1 -maxdepth 1)
  if [ "$(printf '%s\n' "$top" | wc -l)" -ne 1 ] || [ ! -d "$top" ]; then
    echo "itch: $archive does not hold one top folder" >&2
    exit 1
  fi
  mv "$top" "$dir"
  rmdir "$dir.x"
}

pushes=()
stage() {
  local channel=$1 dir=$2 manifest=$3 exe=$4
  if [ ! -e "$dir/$exe" ]; then
    echo "itch: $channel has no $exe" >&2
    exit 1
  fi
  cp "$here/$manifest" "$dir/.itch.toml"
  pushes+=("$channel=$dir")
}

v=$version
missing=0
for f in macos-universal.zip windows-x64.zip windows-x86.zip windows-arm64.zip \
  linux-x86_64.tar.gz linux-aarch64.tar.gz android-arm64.apk; do
  if [ ! -f "$release/froxel-$v-$f" ]; then
    echo "itch: $release/froxel-$v-$f is missing" >&2
    missing=1
  fi
done
[ "$missing" = 0 ] || exit 1

dir="$work/osx-universal"
mkdir -p "$dir"
unzip_clean "$release/froxel-$v-macos-universal.zip" "$dir"
stage osx-universal "$dir" osx.itch.toml Froxel.app/Contents/MacOS/froxel

for arch in x64 x86 arm64; do
  dir="$work/windows-$arch"
  unpack_top "$release/froxel-$v-windows-$arch.zip" "$dir"
  stage "windows-$arch" "$dir" "windows-$arch.itch.toml" froxel.exe
done

for arch in x86_64 aarch64; do
  dir="$work/linux-$arch"
  unpack_top "$release/froxel-$v-linux-$arch.tar.gz" "$dir"
  chmod +x "$dir/froxel"
  stage "linux-$arch" "$dir" linux.itch.toml froxel
done

# An apk is pushed as the file itself; the itch app does not install it.
apk="$release/froxel-$v-android-arm64.apk"
pushes+=("android=$apk")

for entry in "${pushes[@]}"; do
  channel=${entry%%=*}
  path=${entry#*=}
  if [ "${VALIDATE:-}" = 1 ] && [ -d "$path" ]; then
    "$butler" validate "$path"
  fi
  cmd=("$butler" push --if-changed --userversion "$version" "$path" "$project:$channel")
  if [ "${DRY_RUN:-}" = 1 ]; then
    echo "dry run: ${cmd[*]}"
    (cd "$path" 2>/dev/null && find . -type f | sort | sed 's/^/  /') || echo "  $(basename "$path")"
  else
    "${cmd[@]}"
  fi
done
echo "itch: ${#pushes[@]} channels of $project at $version"
