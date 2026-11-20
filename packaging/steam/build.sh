#!/usr/bin/env bash
# Stages a Froxel release into the three Steam depots, renders the SteamPipe
# VDFs, and uploads the build with steamcmd, set live on a beta branch.
#
#   packaging/steam/build.sh <release-dir> <version>
#
# <release-dir> holds the GitHub Release files (froxel-<version>-*). Run from
# the repository root. Environment:
#   STEAM_APP_ID              the Steamworks app ID (required unless DRY_RUN=1)
#   STEAM_DEPOT_WINDOWS_X64   depot IDs; empty means app ID + 1, + 2, + 3
#   STEAM_DEPOT_MACOS
#   STEAM_DEPOT_LINUX_X86_64
#   STEAM_BRANCH              the branch set live (default beta); default and
#                             public are refused
#   STEAM_USERNAME            the build account, logged in with its cached
#                             Steam Guard config.vdf (see the workflow)
#   STEAMCMD                  the steamcmd binary (default steamcmd)
#   STEAM_WORK                where content/, scripts/ and output/ go
#                             (default a temporary folder)
#   DRY_RUN=1                 stage and render, print the steamcmd command,
#                             and stop; without STEAM_APP_ID it uses the
#                             obvious fake ID 1000
set -euo pipefail

release=${1:?release dir}
version=${2:?version}
here=$(cd "$(dirname "$0")" && pwd)
branch=${STEAM_BRANCH:-beta}
steamcmd=${STEAMCMD:-steamcmd}

case "$(printf '%s' "$branch" | tr '[:upper:]' '[:lower:]')" in
default | public | "")
  echo "steam: refusing to set '$branch' live; the default branch goes live by hand in Steamworks" >&2
  exit 1
  ;;
esac
if ! printf '%s' "$branch" | grep -Eq '^[A-Za-z0-9_-]+$'; then
  echo "steam: '$branch' is not a branch name" >&2
  exit 1
fi

app=${STEAM_APP_ID:-}
if [ -z "$app" ]; then
  if [ "${DRY_RUN:-}" != 1 ]; then
    echo "steam: STEAM_APP_ID is not set" >&2
    exit 1
  fi
  app=1000
fi
numeric() {
  if ! printf '%s' "$1" | grep -Eq '^[0-9]+$'; then
    echo "steam: '$1' is not a numeric app or depot ID" >&2
    exit 1
  fi
}
numeric "$app"
win=${STEAM_DEPOT_WINDOWS_X64:-$((app + 1))}
mac=${STEAM_DEPOT_MACOS:-$((app + 2))}
lin=${STEAM_DEPOT_LINUX_X86_64:-$((app + 3))}
for id in "$win" "$mac" "$lin"; do numeric "$id"; done

if [ -n "${STEAM_WORK:-}" ]; then
  work=$STEAM_WORK
  mkdir -p "$work"
else
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT
fi
work=$(cd "$work" && pwd)
content="$work/content"
rm -rf "$content" "$work/scripts"
mkdir -p "$content" "$work/scripts" "$work/output"


# ditto zips may carry AppleDouble files (._*, __MACOSX/) for extended
# attributes; they are not part of the build.
unzip_clean() {
  unzip -q "$1" -d "$2"
  rm -rf "$2/__MACOSX"
  find "$2" -name '._*' -exec rm -rf {} +
}

# The archive's single top folder (froxel-<version>-<platform>/) becomes the
# depot root, so froxel.exe and froxel sit at the top of the install folder.
unpack_top() {
  local archive=$1 dir=$2 top
  mkdir -p "$dir.x"
  case "$archive" in
  *.zip) unzip_clean "$archive" "$dir.x" ;;
  *.tar.gz) tar -C "$dir.x" -xzf "$archive" ;;
  esac
  top=$(find "$dir.x" -mindepth 1 -maxdepth 1)
  if [ "$(printf '%s\n' "$top" | wc -l)" -ne 1 ] || [ ! -d "$top" ]; then
    echo "steam: $archive does not hold one top folder" >&2
    exit 1
  fi
  mv "$top" "$dir"
  rmdir "$dir.x"
}

v=$version
missing=0
for f in windows-x64.zip macos-universal.zip linux-x86_64.tar.gz; do
  if [ ! -f "$release/froxel-$v-$f" ]; then
    echo "steam: $release/froxel-$v-$f is missing" >&2
    missing=1
  fi
done
[ "$missing" = 0 ] || exit 1

unpack_top "$release/froxel-$v-windows-x64.zip" "$content/windows-x64"
mkdir -p "$content/macos-universal"
unzip_clean "$release/froxel-$v-macos-universal.zip" "$content/macos-universal"
unpack_top "$release/froxel-$v-linux-x86_64.tar.gz" "$content/linux-x86_64"
for exe in windows-x64/froxel.exe macos-universal/Froxel.app/Contents/MacOS/froxel linux-x86_64/froxel; do
  if [ ! -f "$content/$exe" ]; then
    echo "steam: the depot has no $exe" >&2
    exit 1
  fi
done
chmod +x "$content/macos-universal/Froxel.app/Contents/MacOS/froxel" "$content/linux-x86_64/froxel"

# Desc is shown in Steamworks' build list; quotes would end the VDF string.
desc="Froxel $version ($(printf '%s' "${GITHUB_SHA:-local}" | cut -c1-12))"
for vdf in app_build.vdf depot_windows_x64.vdf depot_macos.vdf depot_linux_x86_64.vdf; do
  # The comments describe the templates, not the rendered build, so they go.
  sed -e '/^[[:space:]]*\/\//d' \
    -e "s|@STEAM_APP_ID@|$app|g" \
    -e "s|@STEAM_DEPOT_WINDOWS_X64@|$win|g" \
    -e "s|@STEAM_DEPOT_MACOS@|$mac|g" \
    -e "s|@STEAM_DEPOT_LINUX_X86_64@|$lin|g" \
    -e "s|@CONTENT_ROOT@|$content|g" \
    -e "s|@BUILD_OUTPUT@|$work/output|g" \
    -e "s|@BRANCH@|$branch|g" \
    -e "s|@PREVIEW@|0|g" \
    -e "s|@DESC@|$desc|g" \
    "$here/$vdf" >"$work/scripts/$vdf"
  if grep -n '@[A-Z_0-9]*@' "$work/scripts/$vdf"; then
    echo "steam: $vdf still has a placeholder" >&2
    exit 1
  fi
done

echo "steam: app $app, depots $win (Windows x64), $mac (macOS), $lin (Linux x86_64), live on '$branch'"
for d in windows-x64 macos-universal linux-x86_64; do
  echo "  $d: $(find "$content/$d" -type f | wc -l | tr -d ' ') files, $(du -sk "$content/$d" | cut -f1) KB"
done

cmd=("$steamcmd" +login "${STEAM_USERNAME:-<STEAM_USERNAME>}" +run_app_build "$work/scripts/app_build.vdf" +quit)
if [ "${DRY_RUN:-}" = 1 ]; then
  echo "dry run: ${cmd[*]}"
  sed 's/^/  /' "$work/scripts/app_build.vdf"
  exit 0
fi
: "${STEAM_USERNAME:?STEAM_USERNAME}"
"${cmd[@]}"
