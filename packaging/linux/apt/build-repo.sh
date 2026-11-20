#!/usr/bin/env bash
# Builds the signed apt repository that GitHub Pages serves.
#
#   packaging/linux/apt/build-repo.sh <dir with .deb files> <out-dir> <base-url>
#
# Needs apt-ftparchive (apt-utils) and gpg with the signing key imported;
# APT_GPG_KEY_ID picks the key when the keyring has several. The site holds one
# suite, stable, with component main, plus froxel.gpg (the public key), and an
# index.html with the install commands. Each publish replaces the whole site, so
# the repository carries the latest release only.
set -euo pipefail

debs=${1:?debs dir}
out=${2:?out-dir}
base=${3:?base url}
base=${base%/}
here=$(cd "$(dirname "$0")" && pwd)
key=${APT_GPG_KEY_ID:-$(gpg --list-secret-keys --with-colons | awk -F: '$1 == "fpr" { print $10; exit }')}
if [ -z "$key" ]; then
  echo "no gpg secret key" >&2
  exit 1
fi

pool="$out/pool/main/f/froxel"
mkdir -p "$pool"
cp "$debs"/*.deb "$pool/"
cd "$out"

arches=()
for deb in pool/main/f/froxel/*.deb; do
  arches+=("$(dpkg-deb --field "$deb" Architecture)")
done
mapfile -t arches < <(printf '%s\n' "${arches[@]}" | sort -u)

for a in "${arches[@]}"; do
  dir="dists/stable/main/binary-$a"
  mkdir -p "$dir"
  # apt-ftparchive --arch matches nothing here, so filter its stanzas instead.
  apt-ftparchive packages pool | awk -v a="$a" '
    BEGIN { RS = ""; ORS = "\n\n" }
    $0 ~ ("(^|\n)Architecture: (" a "|all)(\n|$)")
  ' >"$dir/Packages"
  gzip -9nk "$dir/Packages"
  xz -9k "$dir/Packages"
done

apt-ftparchive \
  -o APT::FTPArchive::Release::Origin=Froxel \
  -o APT::FTPArchive::Release::Label=Froxel \
  -o APT::FTPArchive::Release::Suite=stable \
  -o APT::FTPArchive::Release::Codename=stable \
  -o APT::FTPArchive::Release::Components=main \
  -o "APT::FTPArchive::Release::Architectures=${arches[*]}" \
  -o APT::FTPArchive::Release::Description="Froxel voxel editor" \
  release dists/stable >dists/stable/Release

gpg_sign=(gpg --batch --yes --pinentry-mode loopback --local-user "$key")
if [ -n "${APT_GPG_PASSPHRASE:-}" ]; then
  gpg_sign+=(--passphrase "$APT_GPG_PASSPHRASE")
fi
"${gpg_sign[@]}" --clearsign -o dists/stable/InRelease dists/stable/Release
"${gpg_sign[@]}" --armor --detach-sign -o dists/stable/Release.gpg dists/stable/Release
gpg --batch --yes --export "$key" >froxel.gpg
gpg --batch --yes --armor --export "$key" >froxel.asc

sed -e "s|@BASE@|$base|g" -e "s|@COPR@|${COPR_PROJECT:-theromis/froxel}|g" "$here/index.html" >index.html
touch .nojekyll
find . -type f | sort
