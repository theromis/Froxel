#!/usr/bin/env bash
# Writes the void-packages template srcpkgs/froxel/template for a release. Void
# builds from source, so it takes the checksum of GitHub's source tarball for
# the tag, which it downloads.
#
#   packaging/void/generate.sh <version> <void-packages-dir>
#
# MAINTAINER (e.g. "Name <email>") is required by void-packages.
set -euo pipefail

version=${1:?version}
out=${2:?void-packages dir}
repo=${REPOSITORY:-theromis/Froxel}
url="https://github.com/$repo"
maintainer=${MAINTAINER:-"MAINTAINER <MAINTAINER_EMAIL>"}

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
curl -fsSL "$url/archive/refs/tags/v$version.tar.gz" -o "$work/src.tar.gz"
checksum=$(sha256sum "$work/src.tar.gz" | cut -d' ' -f1)

mkdir -p "$out/srcpkgs/froxel"
cat >"$out/srcpkgs/froxel/template" <<TEMPLATE
# Template file for 'froxel'
pkgname=froxel
version=$version
revision=1
build_style=cargo
hostmakedepends="pkg-config"
depends="vulkan-loader libxkbcommon"
short_desc="Voxel editor with a path tracer and an MCP server"
maintainer="$maintainer"
license="MIT"
homepage="$url"
changelog="$url/raw/main/CHANGELOG.md"
distfiles="$url/archive/refs/tags/v\${version}.tar.gz"
checksum=$checksum
# The unit tests open sockets and a GPU.
make_check=no

post_install() {
	vlicense LICENSE
	vinstall packaging/linux/froxel.desktop 644 usr/share/applications
	vinstall packaging/linux/com.lonje.Froxel.metainfo.xml 644 usr/share/metainfo
	for png in packaging/icons/hicolor/*.png; do
		vinstall "\${png}" 644 "usr/share/icons/hicolor/\$(basename "\${png}" .png)/apps" froxel.png
	done
}
TEMPLATE
echo "wrote $out/srcpkgs/froxel/template"
