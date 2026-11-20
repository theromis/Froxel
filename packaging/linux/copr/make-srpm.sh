#!/usr/bin/env bash
# Builds the COPR and openSUSE Build Service source rpm from a release's Linux tarballs.
#
#   packaging/linux/copr/make-srpm.sh <version> <dir with the tarballs> <out-dir>
#
# Needs rpmbuild. The source rpm goes to <out-dir>, which `copr-cli build`
# takes, with the filled-in froxel.spec, which the openSUSE Build Service takes.
set -euo pipefail

version=${1:?version}
assets=${2:?assets dir}
out=${3:?out-dir}
repo=${REPOSITORY:-theromis/Froxel}
here=$(cd "$(dirname "$0")" && pwd)

top=$(mktemp -d)
trap 'rm -rf "$top"' EXIT
mkdir -p "$top/SOURCES" "$top/SPECS" "$out"
for a in x86_64 aarch64 riscv64; do
  cp "$assets/froxel-$version-linux-$a.tar.gz" "$top/SOURCES/"
done
# rpm sorts 1.0~rc.1 before 1.0, as semver sorts 1.0-rc.1.
sed -e "s/@UPSTREAM_VERSION@/$version/" \
  -e "s/@RPM_VERSION@/$(printf '%s' "$version" | tr - '~')/" \
  -e "s|@REPOSITORY@|$repo|" \
  "$here/froxel.spec.in" >"$top/SPECS/froxel.spec"
rpmbuild -bs --define "_topdir $top" "$top/SPECS/froxel.spec"
cp "$top"/SRPMS/*.src.rpm "$out/"
# The spec and the tarballs on their own, for the openSUSE Build Service.
cp "$top/SPECS/froxel.spec" "$out/"
ls "$out"/*.src.rpm
