#!/usr/bin/env bash
# Writes the FreeBSD port graphics/froxel for a release tag: Makefile,
# Makefile.crates (every crates.io crate in that tag's Cargo.lock), and
# pkg-descr. On FreeBSD, `make makesum` in the port then writes distinfo, and
# `make stage check-plist package` builds it (docs/releasing.md, "FreeBSD").
#
#   packaging/freebsd/generate.sh <version> <ports-dir>
#
# MAINTAINER_EMAIL is the port maintainer (default ports@FreeBSD.org, the
# ports tree's address for a port nobody maintains). GH_TAGNAME
# builds that commit instead of the tag v<version> (to test a port before the
# release is tagged; <version> should then be the commit's Cargo.toml version).
set -euo pipefail

version=${1:?version}
ports=${2:?ports dir}
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
dir="$ports/graphics/froxel"
mkdir -p "$dir"

ref=${GH_TAGNAME:-v$version}
lock=$(git -C "$root" show "$ref:Cargo.lock" 2>/dev/null || cat "$root/Cargo.lock")
{
  echo "CARGO_CRATES=	\\"
  printf '%s\n' "$lock" | awk '
    /^\[\[package\]\]/ { name = ""; ver = ""; reg = 0 }
    /^name = / { gsub(/"/, "", $3); name = $3 }
    /^version = / { gsub(/"/, "", $3); ver = $3 }
    /^source = "registry\+/ { reg = 1 }
    /^$/ { if (reg) print name "-" ver; reg = 0 }
    END { if (reg) print name "-" ver }
  ' | sort -u | sed 's/^/		/; s/$/ \\/' | sed '$ s/ \\$//'
} >"$dir/Makefile.crates"

cat >"$dir/Makefile" <<MAKE
PORTNAME=	froxel
DISTVERSIONPREFIX=	v
DISTVERSION=	$version
CATEGORIES=	graphics

MAINTAINER=	${MAINTAINER_EMAIL:-ports@FreeBSD.org}
COMMENT=	Voxel editor with a path tracer and an MCP server
WWW=		https://github.com/theromis/Froxel

LICENSE=	MIT
LICENSE_FILE=	\${WRKSRC}/LICENSE

# Vulkan, xkbcommon, and Wayland are opened at run time, not linked.
RUN_DEPENDS=	vulkan-loader>0:graphics/vulkan-loader \\
		libxkbcommon>0:x11/libxkbcommon \\
		wayland>0:graphics/wayland

USES=		cargo
USE_GITHUB=	yes
GH_ACCOUNT=	theromis
GH_PROJECT=	Froxel${GH_TAGNAME:+
GH_TAGNAME=	$GH_TAGNAME}

PLIST_FILES=	bin/froxel \\
		share/applications/froxel.desktop \\
		share/icons/hicolor/128x128/apps/froxel.png \\
		share/icons/hicolor/16x16/apps/froxel.png \\
		share/icons/hicolor/256x256/apps/froxel.png \\
		share/icons/hicolor/32x32/apps/froxel.png \\
		share/icons/hicolor/48x48/apps/froxel.png \\
		share/icons/hicolor/512x512/apps/froxel.png \\
		share/icons/hicolor/64x64/apps/froxel.png \\
		share/metainfo/com.lonje.Froxel.metainfo.xml

post-install:
	\${INSTALL_DATA} \${WRKSRC}/packaging/linux/froxel.desktop \${STAGEDIR}\${PREFIX}/share/applications/
	@\${MKDIR} \${STAGEDIR}\${PREFIX}/share/metainfo
	\${INSTALL_DATA} \${WRKSRC}/packaging/linux/com.lonje.Froxel.metainfo.xml \${STAGEDIR}\${PREFIX}/share/metainfo/
.for size in 16x16 32x32 48x48 64x64 128x128 256x256 512x512
	@\${MKDIR} \${STAGEDIR}\${PREFIX}/share/icons/hicolor/\${size}/apps
	\${INSTALL_DATA} \${WRKSRC}/packaging/icons/hicolor/\${size}.png \\
		\${STAGEDIR}\${PREFIX}/share/icons/hicolor/\${size}/apps/froxel.png
.endfor

.include <bsd.port.mk>
MAKE

cat >"$dir/pkg-descr" <<'DESCR'
Froxel is a voxel editor for MagicaVoxel .vox models. Sculpt with brushes,
lines, boxes, and faces, rig models into parts with bones and clips, and see
them in a GPU path tracer. It imports glTF and OBJ meshes, exports glTF, and
its MCP server lets an AI agent build and edit models.
DESCR
echo "wrote $dir ($(grep -c '^		' "$dir/Makefile.crates") crates)"
