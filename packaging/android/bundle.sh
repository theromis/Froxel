#!/bin/sh
# Turns the APK that cargo-apk built into an Android App Bundle for Google
# Play, which takes only .aab uploads.
#
#   cargo apk build --lib --features android-lib --release
#   packaging/android/bundle.sh                       # target/release/apk/froxel.aab
#   KEYSTORE=froxel.keystore KEYSTORE_PASSWORD=... packaging/android/bundle.sh
#
# It reuses cargo-apk's AndroidManifest.xml, resources, and libfroxel.so, links
# them again in aapt2's proto format, and has bundletool build the bundle.
# versionCode is the store build number from packaging/build_number.sh, and
# versionName is VERSION (default: Cargo.toml). With KEYSTORE and
# KEYSTORE_PASSWORD the bundle is signed with the keystore's only key, the
# Play upload key; without them it is unsigned and Play rejects it.
#
# Needs a JDK (java, jarsigner), ANDROID_HOME with build-tools and the
# platform of target_sdk_version, and bundletool: BUNDLETOOL names the jar,
# or the pinned release is downloaded to target/bundletool/.
# Prints the path to the .aab.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
apk_dir="$root/target/release/apk"
version=${VERSION:-$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)}
code=$("$root/packaging/build_number.sh" "$version")
sdk=$(sed -n 's/^target_sdk_version = \([0-9]*\)$/\1/p' "$root/Cargo.toml" | head -n 1)
android_home=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}
android_jar="$android_home/platforms/android-$sdk/android.jar"
tools=$(find "$android_home/build-tools" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)

if [ ! -f "$apk_dir/froxel.apk" ] || [ ! -f "$apk_dir/AndroidManifest.xml" ]; then
  echo "bundle.sh: run 'cargo apk build --lib --features android-lib --release' first" >&2
  exit 1
fi
if [ ! -f "$android_jar" ]; then
  echo "bundle.sh: $android_jar is missing; install platforms;android-$sdk" >&2
  exit 1
fi

bundletool_version=1.18.3
bundletool_sha256=a099cfa1543f55593bc2ed16a70a7c67fe54b1747bb7301f37fdfd6d91028e29
bundletool=${BUNDLETOOL:-$root/target/bundletool/bundletool-all-$bundletool_version.jar}
if [ ! -f "$bundletool" ]; then
  mkdir -p "$(dirname "$bundletool")"
  curl -sSfL -o "$bundletool.part" \
    "https://github.com/google/bundletool/releases/download/$bundletool_version/bundletool-all-$bundletool_version.jar"
  if [ "$(shasum -a 256 "$bundletool.part" | cut -d' ' -f1)" != "$bundletool_sha256" ]; then
    rm -f "$bundletool.part"
    echo "bundle.sh: bundletool $bundletool_version does not match its SHA-256" >&2
    exit 1
  fi
  mv "$bundletool.part" "$bundletool"
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Resources and manifest in proto format, with the store version.
"$tools/aapt2" compile --dir "$root/packaging/android/res" -o "$work/res.zip"
"$tools/aapt2" link --proto-format -o "$work/linked.apk" -I "$android_jar" \
  --manifest "$apk_dir/AndroidManifest.xml" --auto-add-overlay \
  --version-code "$code" --version-name "$version" --replace-version \
  "$work/res.zip"

# A bundle module: manifest/, res/, resources.pb, and lib/ from cargo-apk's APK.
mkdir -p "$work/base/manifest"
(cd "$work/base" && unzip -q "$work/linked.apk")
mv "$work/base/AndroidManifest.xml" "$work/base/manifest/"
(cd "$work/base" && unzip -q "$apk_dir/froxel.apk" 'lib/*')
(cd "$work/base" && zip -qr -X "$work/base.zip" .)

aab="$apk_dir/froxel.aab"
rm -f "$aab"
java -jar "$bundletool" build-bundle --modules="$work/base.zip" --output="$aab" >&2

if [ -n "${KEYSTORE:-}" ]; then
  alias=$(keytool -list -keystore "$KEYSTORE" -storepass "$KEYSTORE_PASSWORD" \
    | sed -n 's/^\([^,]*\),.*PrivateKeyEntry.*/\1/p' | head -n 1)
  if [ -z "$alias" ]; then
    echo "bundle.sh: $KEYSTORE has no private key" >&2
    exit 1
  fi
  jarsigner -keystore "$KEYSTORE" -storepass "$KEYSTORE_PASSWORD" \
    -sigalg SHA256withRSA -digestalg SHA-256 "$aab" "$alias" >&2
  jarsigner -verify "$aab" >&2
else
  echo "bundle.sh: no KEYSTORE; the bundle is unsigned" >&2
fi

java -jar "$bundletool" validate --bundle="$aab" >/dev/null
echo "$aab"
