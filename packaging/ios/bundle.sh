#!/bin/sh
# Builds Froxel for the iOS simulator and wraps it in Froxel.app.
#
#   packaging/ios/bundle.sh                  # debug, aarch64-apple-ios-sim
#   PROFILE=release packaging/ios/bundle.sh
#   TARGET=aarch64-apple-ios PROFILE=release packaging/ios/bundle.sh   # device
#
# Needs Xcode and the rustup toolchain with the aarch64-apple-ios-sim target.
# actool compiles the icon from packaging/icons/froxel-ios.png, and the
# Info.plist names the SDK and Xcode it was built with, as App Store Connect
# expects. The app gets an ad-hoc signature, which the simulator accepts; a device build needs
# a provisioning profile. VERSION overrides the version from Cargo.toml.
# Prints the path to the .app.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
target=${TARGET:-aarch64-apple-ios-sim}
profile=${PROFILE:-debug}
version=${VERSION:-$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)}
case "$target" in
*-sim) platform=iPhoneSimulator sdk=iphonesimulator ;;
*) platform=iPhoneOS sdk=iphoneos ;;
esac
sdk_version=$(xcrun --sdk "$sdk" --show-sdk-version)
sdk_build=$(xcrun --sdk "$sdk" --show-sdk-build-version)
# Xcode 16.4 is 1640 and 26.0.1 is 2601.
xcode=$(xcodebuild -version | sed -n 's/^Xcode //p' | awk -F. '{ printf "%d%d%d", $1, $2, $3 }')
xcode_build=$(xcodebuild -version | sed -n 's/^Build version //p')

flag=""
if [ "$profile" = release ]; then flag=--release; fi
# rustup's rustc has the iOS standard library; a system rustc may not.
PATH="$HOME/.cargo/bin:$PATH" cargo build --manifest-path "$root/Cargo.toml" --target "$target" $flag >&2

app="$root/target/$target/$profile/Froxel.app"
rm -rf "$app"
mkdir -p "$app"
cp "$root/target/$target/$profile/froxel" "$app/froxel"
icons=$(mktemp -d)
trap 'rm -rf "$icons"' EXIT
mkdir -p "$icons/Assets.xcassets/AppIcon.appiconset"
cp "$root/packaging/icons/froxel-ios.png" "$icons/Assets.xcassets/AppIcon.appiconset/"
cat >"$icons/Assets.xcassets/AppIcon.appiconset/Contents.json" <<EOF
{
  "images": [
    { "filename": "froxel-ios.png", "idiom": "universal", "platform": "ios", "size": "1024x1024" }
  ],
  "info": { "author": "xcode", "version": 1 }
}
EOF
xcrun actool --compile "$app" --platform "$sdk" --minimum-deployment-target 16.0 \
  --target-device iphone --target-device ipad --app-icon AppIcon \
  --output-partial-info-plist "$icons/icons.plist" --output-format human-readable-text \
  "$icons/Assets.xcassets" >&2
cp "$root/examples/frog.vox" "$root/examples/lily_pad.vox" "$app/"
cat >"$app/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Froxel</string>
  <key>CFBundleDisplayName</key><string>Froxel</string>
  <key>CFBundleIdentifier</key><string>com.lonje.froxel</string>
  <key>CFBundleExecutable</key><string>froxel</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${version%%-*}</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleSupportedPlatforms</key><array><string>$platform</string></array>
  <key>DTPlatformName</key><string>$sdk</string>
  <key>DTPlatformVersion</key><string>$sdk_version</string>
  <key>DTPlatformBuild</key><string>$sdk_build</string>
  <key>DTSDKName</key><string>$sdk$sdk_version</string>
  <key>DTSDKBuild</key><string>$sdk_build</string>
  <key>DTXcode</key><string>$xcode</string>
  <key>DTXcodeBuild</key><string>$xcode_build</string>
  <key>DTCompiler</key><string>com.apple.compilers.llvm.clang.1_0</string>
  <key>BuildMachineOSBuild</key><string>$(sw_vers -buildVersion)</string>
  <key>UIRequiredDeviceCapabilities</key><array><string>arm64</string></array>
  <key>ITSAppUsesNonExemptEncryption</key><false/>
  <key>MinimumOSVersion</key><string>16.0</string>
  <key>UIDeviceFamily</key><array><integer>1</integer><integer>2</integer></array>
  <key>UILaunchScreen</key><dict/>
  <key>UIRequiresFullScreen</key><true/>
  <key>UIStatusBarHidden</key><true/>
  <key>UIFileSharingEnabled</key><true/>
  <key>LSSupportsOpeningDocumentsInPlace</key><true/>
  <key>UISupportedInterfaceOrientations</key>
  <array>
    <string>UIInterfaceOrientationPortrait</string>
    <string>UIInterfaceOrientationLandscapeLeft</string>
    <string>UIInterfaceOrientationLandscapeRight</string>
  </array>
  <key>UISupportedInterfaceOrientations~ipad</key>
  <array>
    <string>UIInterfaceOrientationPortrait</string>
    <string>UIInterfaceOrientationPortraitUpsideDown</string>
    <string>UIInterfaceOrientationLandscapeLeft</string>
    <string>UIInterfaceOrientationLandscapeRight</string>
  </array>
</dict>
</plist>
EOF
/usr/libexec/PlistBuddy -c "Merge $icons/icons.plist" "$app/Info.plist" >&2
plutil -lint "$app/Info.plist" >&2
codesign --force --sign - "$app" >&2
echo "$app"
