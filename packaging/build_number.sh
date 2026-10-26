#!/bin/sh
# Prints the store build number for a version: the iOS CFBundleVersion and the
# Android versionCode. Both stores want an integer that grows with each upload.
#
#   packaging/build_number.sh 0.2.0-rc.1   # 20001
#   packaging/build_number.sh 0.2.0        # 20099
#   packaging/build_number.sh              # version from Cargo.toml
#
# The number is major * 1000000 + minor * 10000 + patch * 100 + stage, where
# the stage is the last number of a prerelease (1 to 98) and 99 for a release.
# So 0.2.0-rc.3 sorts after 0.2.0-rc.2 and before 0.2.0. Number prereleases
# across their kinds (alpha.1, beta.2, rc.3): beta.1 after alpha.2 would go
# back. Minor and patch stay below 100.
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
version=${1:-$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)}
version=${version#v}

core=${version%%-*}
case "$core" in
*[!0-9.]* | *..* | .* | *.) core= ;;
esac
major=$(printf '%s' "$core" | cut -d. -f1)
minor=$(printf '%s' "$core" | cut -d. -f2)
patch=$(printf '%s' "$core" | cut -d. -f3)
if [ -z "$core" ] || [ "$(printf '%s' "$core" | tr -cd . | wc -c)" -ne 2 ]; then
  echo "build_number: '$version' is not a version like 1.2.3 or 1.2.3-rc.1" >&2
  exit 1
fi
if [ "$minor" -ge 100 ] || [ "$patch" -ge 100 ] || [ "$major" -ge 2000 ]; then
  echo "build_number: $version does not fit (minor and patch below 100)" >&2
  exit 1
fi

if [ "$core" = "$version" ]; then
  stage=99
else
  stage=$(printf '%s' "${version#*-}" | grep -Eo '[1-9][0-9]*$' || true)
  if [ -z "$stage" ] || [ "$stage" -lt 1 ] || [ "$stage" -gt 98 ]; then
    echo "build_number: the prerelease of $version must end in a number from 1 to 98, as in rc.1" >&2
    exit 1
  fi
fi

echo $((major * 1000000 + minor * 10000 + patch * 100 + stage))
