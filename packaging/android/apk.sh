#!/bin/sh
# Rebuilds the APK that cargo-apk made with the store versionCode, for the
# release download, F-Droid, the Amazon Appstore, and the Samsung Galaxy Store.
#
#   cargo apk build --lib --features android-lib --release
#   packaging/android/apk.sh                       # target/release/apk/froxel-store.apk
#   KEYSTORE=froxel.keystore KEYSTORE_PASSWORD=... packaging/android/apk.sh
#
# cargo-apk packs versionCode from the version itself (1 << 24 | major << 16 |
# minor << 8 | patch), so a prerelease and its release get the same code, and
# it differs from the App Bundle's. This script links cargo-apk's
# AndroidManifest.xml and resources again with versionCode from
# packaging/build_number.sh and versionName VERSION (default: Cargo.toml),
# adds lib/ from cargo-apk's APK uncompressed, and aligns the native library
# to 16 KB pages. With KEYSTORE and KEYSTORE_PASSWORD it is signed with the
# keystore's only key; without them it is unsigned, which is what F-Droid's
# build server signs.
#
# Needs ANDROID_HOME with build-tools and the platform of target_sdk_version,
# zip and unzip, and a JDK for apksigner when signing. APK_DIR is cargo-apk's
# output folder (default: target/release/apk). Prints the path to the APK.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
apk_dir=${APK_DIR:-$root/target/release/apk}
version=${VERSION:-$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)}
code=$("$root/packaging/build_number.sh" "$version")
sdk=$(sed -n 's/^target_sdk_version = \([0-9]*\)$/\1/p' "$root/Cargo.toml" | head -n 1)
android_home=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}
android_jar="$android_home/platforms/android-$sdk/android.jar"
tools=$(find "$android_home/build-tools" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)

if [ ! -f "$apk_dir/froxel.apk" ] || [ ! -f "$apk_dir/AndroidManifest.xml" ]; then
  echo "apk.sh: run 'cargo apk build --lib --features android-lib --release' first" >&2
  exit 1
fi
if [ ! -f "$android_jar" ]; then
  echo "apk.sh: $android_jar is missing; install platforms;android-$sdk" >&2
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Manifest and resources with the store version, in the binary APK format.
"$tools/aapt2" compile --dir "$root/packaging/android/res" -o "$work/res.zip"
"$tools/aapt2" link -o "$work/linked.apk" -I "$android_jar" \
  --manifest "$apk_dir/AndroidManifest.xml" --auto-add-overlay \
  --version-code "$code" --version-name "$version" --replace-version \
  "$work/res.zip"

# The native library from cargo-apk's APK, stored so that it can be aligned.
mkdir -p "$work/libs"
(cd "$work/libs" && unzip -q "$apk_dir/froxel.apk" 'lib/*')
(cd "$work/libs" && zip -q -0 -r -X "$work/linked.apk" lib)

out="$apk_dir/froxel-store.apk"
rm -f "$out"
"$tools/zipalign" -P 16 -f 4 "$work/linked.apk" "$out"
"$tools/zipalign" -c -P 16 4 "$out"

if [ -n "${KEYSTORE:-}" ]; then
  "$tools/apksigner" sign --ks "$KEYSTORE" --ks-pass env:KEYSTORE_PASSWORD "$out" >&2
  "$tools/apksigner" verify "$out" >&2
else
  echo "apk.sh: no KEYSTORE; the APK is unsigned" >&2
fi

echo "$out"
