#!/bin/sh
# Wraps built froxel binaries in the sandboxed Mac App Store Froxel.app and,
# with an installer identity, the signed .pkg that App Store Connect takes.
#
#   packaging/macos/appstore.sh                       # target/release/froxel, ad-hoc
#   packaging/macos/appstore.sh arm64/froxel x86_64/froxel   # lipo'd universal
#
# Environment:
#   VERSION             the version; default Cargo.toml
#   OUT                 the output folder; default target/appstore
#   SIGN_IDENTITY       "Apple Distribution: …" or "3rd Party Mac Developer
#                       Application: …"; default "-", an ad-hoc signature
#   PROFILE             the Mac App Store provisioning profile
#                       (.provisionprofile); needed with SIGN_IDENTITY
#   INSTALLER_IDENTITY  "3rd Party Mac Developer Installer: …" or "Mac Installer
#                       Distribution: …"; makes OUT/froxel-<version>-macos.pkg
#   KEYCHAIN            the keychain that holds the identities
#
# The app is the Developer ID app plus the App Sandbox entitlements in
# Froxel.entitlements (with the profile's application and team identifiers),
# CFBundleVersion from packaging/build_number.sh, an asset catalog icon, the
# SDK and Xcode keys App Store Connect checks, ITSAppUsesNonExemptEncryption
# false, and the privacy manifest. An ad-hoc build runs sandboxed on this Mac,
# for testing; only a profile-signed one is accepted by the store.
# Prints the app path, and the pkg path when one is made.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
version=${VERSION:-$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)}
build=$("$root/packaging/build_number.sh" "$version")
out=${OUT:-$root/target/appstore}
identity=${SIGN_IDENTITY:--}
minimum=${MACOSX_DEPLOYMENT_TARGET:-11.0}
if [ "$#" -eq 0 ]; then
  set -- "$root/target/release/froxel"
fi
if [ "$identity" != - ] && [ -z "${PROFILE:-}" ]; then
  echo "appstore.sh: SIGN_IDENTITY needs PROFILE, the Mac App Store provisioning profile" >&2
  exit 1
fi
keychain=""
if [ -n "${KEYCHAIN:-}" ]; then keychain="--keychain $KEYCHAIN"; fi

sdk_version=$(xcrun --sdk macosx --show-sdk-version)
sdk_build=$(xcrun --sdk macosx --show-sdk-build-version)
# Xcode 16.4 is 1640 and 26.0.1 is 2601.
xcode=$(xcodebuild -version | sed -n 's/^Xcode //p' | awk -F. '{ printf "%d%d%d", $1, $2, $3 }')
xcode_build=$(xcodebuild -version | sed -n 's/^Build version //p')

app="$out/Froxel.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
lipo -create -output "$app/Contents/MacOS/froxel" "$@"
lipo -info "$app/Contents/MacOS/froxel" >&2
cp "$root/packaging/icons/Froxel.icns" "$app/Contents/Resources/"
cp "$root/packaging/ios/PrivacyInfo.xcprivacy" "$app/Contents/Resources/"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
icons="$work/Assets.xcassets/AppIcon.appiconset"
mkdir -p "$icons"
images=""
for size in 16 32 128 256 512; do
  for scale in 1 2; do
    px=$((size * scale))
    file="icon_${size}x${size}@${scale}x.png"
    sips -z "$px" "$px" "$root/packaging/icons/froxel-macos.png" --out "$icons/$file" >/dev/null
    images="$images{ \"filename\": \"$file\", \"idiom\": \"mac\", \"scale\": \"${scale}x\", \"size\": \"${size}x${size}\" },"
  done
done
cat >"$icons/Contents.json" <<EOF
{ "images": [ ${images%,} ], "info": { "author": "xcode", "version": 1 } }
EOF
xcrun actool --compile "$app/Contents/Resources" --platform macosx \
  --minimum-deployment-target "$minimum" --app-icon AppIcon \
  --output-partial-info-plist "$work/icons.plist" --output-format human-readable-text \
  "$work/Assets.xcassets" >&2

cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Froxel</string>
  <key>CFBundleDisplayName</key><string>Froxel</string>
  <key>CFBundleIdentifier</key><string>com.lonje.froxel</string>
  <key>CFBundleExecutable</key><string>froxel</string>
  <key>CFBundleIconFile</key><string>Froxel</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version%%-*}</string>
  <key>CFBundleVersion</key><string>$build</string>
  <key>CFBundleSupportedPlatforms</key><array><string>MacOSX</string></array>
  <key>CFBundleDevelopmentRegion</key><string>en</string>
  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>
  <key>DTPlatformName</key><string>macosx</string>
  <key>DTPlatformVersion</key><string>$sdk_version</string>
  <key>DTPlatformBuild</key><string>$sdk_build</string>
  <key>DTSDKName</key><string>macosx$sdk_version</string>
  <key>DTSDKBuild</key><string>$sdk_build</string>
  <key>DTXcode</key><string>$xcode</string>
  <key>DTXcodeBuild</key><string>$xcode_build</string>
  <key>DTCompiler</key><string>com.apple.compilers.llvm.clang.1_0</string>
  <key>BuildMachineOSBuild</key><string>$(sw_vers -buildVersion)</string>
  <key>LSMinimumSystemVersion</key><string>$minimum</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.graphics-design</string>
  <key>ITSAppUsesNonExemptEncryption</key><false/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
EOF
/usr/libexec/PlistBuddy -c "Merge $work/icons.plist" "$app/Contents/Info.plist" >&2
plutil -lint "$app/Contents/Info.plist" >&2

entitlements="$work/entitlements.plist"
cp "$root/packaging/macos/Froxel.entitlements" "$entitlements"
if [ -n "${PROFILE:-}" ]; then
  security cms -D -i "$PROFILE" >"$work/profile.plist"
  app_id=$(plutil -extract Entitlements.com\\.apple\\.application-identifier raw "$work/profile.plist")
  team=$(plutil -extract Entitlements.com\\.apple\\.developer\\.team-identifier raw "$work/profile.plist")
  case "$app_id" in
  "$team".com.lonje.froxel) ;;
  *)
    echo "appstore.sh: the profile is for $app_id, not $team.com.lonje.froxel" >&2
    exit 1
    ;;
  esac
  # A Developer ID or development profile lists devices; a store profile does not.
  if plutil -extract ProvisionedDevices raw "$work/profile.plist" >/dev/null 2>&1 ||
    plutil -extract ProvisionsAllDevices raw "$work/profile.plist" >/dev/null 2>&1; then
    echo "appstore.sh: $(plutil -extract Name raw "$work/profile.plist") is not a Mac App Store profile" >&2
    exit 1
  fi
  plutil -insert com\\.apple\\.application-identifier -string "$app_id" "$entitlements"
  plutil -insert com\\.apple\\.developer\\.team-identifier -string "$team" "$entitlements"
  cp "$PROFILE" "$app/Contents/embedded.provisionprofile"
fi
plutil -lint "$entitlements" >&2

# shellcheck disable=SC2086 # $keychain is empty or two words.
codesign --force --sign "$identity" $keychain --entitlements "$entitlements" "$app" >&2
codesign --verify --strict --verbose=2 "$app" >&2
codesign --display --entitlements - "$app" >&2
echo "$app"

if [ -n "${INSTALLER_IDENTITY:-}" ]; then
  pkg="$out/froxel-$version-macos.pkg"
  # shellcheck disable=SC2086
  productbuild --component "$app" /Applications --sign "$INSTALLER_IDENTITY" $keychain "$pkg" >&2
  pkgutil --check-signature "$pkg" >&2
  echo "$pkg"
fi
