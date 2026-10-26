#!/bin/sh
# Builds Froxel for the iOS simulator and wraps it in Froxel.app.
#
#   packaging/ios/bundle.sh                  # debug, aarch64-apple-ios-sim
#   PROFILE=release packaging/ios/bundle.sh
#   TARGET=aarch64-apple-ios PROFILE=release packaging/ios/bundle.sh   # device
#
# Needs the rustup toolchain with the aarch64-apple-ios-sim target. The app
# gets an ad-hoc signature, which the simulator accepts; a device build needs
# a provisioning profile. VERSION overrides the version from Cargo.toml.
# Prints the path to the .app.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
target=${TARGET:-aarch64-apple-ios-sim}
profile=${PROFILE:-debug}
version=${VERSION:-$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)}
case "$target" in
*-sim) platform=iPhoneSimulator ;;
*) platform=iPhoneOS ;;
esac

flag=""
if [ "$profile" = release ]; then flag=--release; fi
# rustup's rustc has the iOS standard library; a system rustc may not.
PATH="$HOME/.cargo/bin:$PATH" cargo build --manifest-path "$root/Cargo.toml" --target "$target" $flag >&2

app="$root/target/$target/$profile/Froxel.app"
rm -rf "$app"
mkdir -p "$app"
cp "$root/target/$target/$profile/froxel" "$app/froxel"
cp "$root/packaging/icons/hicolor/256x256.png" "$app/AppIcon.png"
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
  <key>CFBundleIconFiles</key><array><string>AppIcon.png</string></array>
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
plutil -lint "$app/Info.plist" >&2
codesign --force --sign - "$app" >&2
echo "$app"
