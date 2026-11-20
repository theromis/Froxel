#!/usr/bin/env bash
# Packages a built Linux binary as a .tar.gz, a .deb and an .rpm.
#
#   VERSION=0.3.3 packaging/linux/package.sh <arch> <path/to/froxel> [out-dir]
#
# <arch> is x86_64, aarch64 or riscv64. Run from the repository root. Needs
# dpkg-deb, rpmbuild and an objdump that reads the binary (OBJDUMP, default
# objdump). The minimum glibc in the package dependencies is read from the
# binary's versioned symbols, so it follows the system the binary was built on.
set -euo pipefail

arch=${1:?arch}
bin=${2:?binary}
out=${3:-dist/release}
: "${VERSION:?VERSION}"
objdump=${OBJDUMP:-objdump}
homepage=${HOMEPAGE:-https://github.com/theromis/Froxel}

case "$arch" in
x86_64) deb_arch=amd64 ;;
aarch64) deb_arch=arm64 ;;
riscv64) deb_arch=riscv64 ;;
*)
  echo "unknown arch $arch" >&2
  exit 1
  ;;
esac

glibc=$("$objdump" -T "$bin" | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/GLIBC_//' | sort -uV | tail -n1)
if [ -z "$glibc" ]; then
  echo "no GLIBC symbol versions in $bin" >&2
  exit 1
fi
echo "$bin needs glibc $glibc"

name="froxel-$VERSION-linux-$arch"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir -p "$out"
out=$(cd "$out" && pwd)

# The tarball: the binary, the desktop entry, and the icon.
mkdir -p "$work/$name"
cp "$bin" README.md LICENSE packaging/linux/froxel.desktop packaging/icons/froxel.png "$work/$name/"
tar -C "$work" -czf "$out/$name.tar.gz" "$name"

# The file tree that both packages install.
root="$work/root"
install -Dm755 "$bin" "$root/usr/bin/froxel"
install -Dm644 packaging/linux/froxel.desktop "$root/usr/share/applications/froxel.desktop"
for png in packaging/icons/hicolor/*.png; do
  size=$(basename "$png" .png)
  install -Dm644 "$png" "$root/usr/share/icons/hicolor/$size/apps/froxel.png"
done
install -Dm644 LICENSE "$root/usr/share/licenses/froxel/LICENSE"
install -Dm644 README.md "$root/usr/share/doc/froxel/README.md"

# Debian and Ubuntu. Debian sorts 1.0~rc1 before 1.0, as semver sorts 1.0-rc1;
# rpm does the same with the tilde.
pkg_version=$(printf '%s' "$VERSION" | tr - '~')
deb="$work/deb"
cp -a "$root" "$deb"
install -Dm644 LICENSE "$deb/usr/share/doc/froxel/copyright"
rm -r "$deb/usr/share/licenses"
mkdir -p "$deb/DEBIAN"
cat >"$deb/DEBIAN/control" <<EOF
Package: froxel
Version: $pkg_version
Architecture: $deb_arch
Maintainer: Froxel <noreply@github.com>
Section: graphics
Priority: optional
Installed-Size: $(du -sk "$deb/usr" | cut -f1)
Depends: libc6 (>= $glibc), libgcc-s1 | libgcc1, libvulkan1, libxkbcommon0
Recommends: libwayland-client0, libxkbcommon-x11-0, libx11-6, xdg-desktop-portal
Homepage: $homepage
Description: Voxel editor
 A voxel editor in the spirit of MagicaVoxel, with a wgpu viewport,
 a path tracer and an MCP server for automation.
EOF
dpkg-deb --root-owner-group -Zxz --build "$deb" "$out/$name.deb"
dpkg-deb --info "$out/$name.deb"

# Fedora, RHEL, CentOS Stream, Rocky, Alma, openSUSE. The libraries are named by
# soname, which every rpm distribution provides under its own package names.
# rpmbuild adds the glibc and libgcc dependencies from the binary itself.
top="$work/rpm"
mkdir -p "$top/SPECS" "$top/RPMS"
cat >"$top/SPECS/froxel.spec" <<EOF
%global debug_package %{nil}
%global __os_install_post %{nil}
%define _build_id_links none
Name: froxel
Version: $pkg_version
Release: 1
Summary: Voxel editor
License: MIT
URL: $homepage
Requires: libvulkan.so.1()(64bit)
Requires: libxkbcommon.so.0()(64bit)
Recommends: libwayland-client.so.0()(64bit)
Recommends: libxkbcommon-x11.so.0()(64bit)
Recommends: libX11.so.6()(64bit)
Recommends: xdg-desktop-portal

%description
A voxel editor in the spirit of MagicaVoxel, with a wgpu viewport,
a path tracer and an MCP server for automation.

%install
cp -a "$root/." %{buildroot}/

%files
%license /usr/share/licenses/froxel/LICENSE
%doc /usr/share/doc/froxel/README.md
/usr/bin/froxel
/usr/share/applications/froxel.desktop
/usr/share/icons/hicolor/*/apps/froxel.png
EOF
rpmbuild -bb --target "$arch" --define "_topdir $top" "$top/SPECS/froxel.spec"
cp "$top"/RPMS/*/froxel-*.rpm "$out/$name.rpm"
rpm -qip "$out/$name.rpm"
rpm -qRp "$out/$name.rpm"
