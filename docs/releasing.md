# Releasing

Before a release, move the `Unreleased` entries in `CHANGELOG.md` under the
new version and set `version` in `Cargo.toml`.

`.github/workflows/release.yml` builds every platform. It runs only on request:

```sh
gh workflow run release.yml                      # version from Cargo.toml
gh workflow run release.yml -f version=0.1.0-rc.1
gh workflow run release.yml -f draft-release=false  # artifacts only
gh workflow run release.yml -f upload-stores=true   # also TestFlight and Google Play
```

It builds macOS universal (`.dmg`, `.zip`), Windows x64, x86 and arm64
(`.zip`), Linux x86_64, aarch64 and riscv64 (`.tar.gz`, `.deb`) and FreeBSD
x86_64 (`.tar.gz`), Android arm64 (`.apk`) and iOS (`.ipa` for devices, a
zipped `Froxel.app` for the simulator), then creates or updates the draft release `v<version>`
with a `SHA256SUMS.txt`. It also zips the Godot plugin and the Blender add-on.
The Android App Bundle and an App Store-signed ipa are workflow artifacts
named `store-<version>-android` and `store-<version>-ios`, not release files;
see [Store builds](#store-builds).
The release notes are the version's section of `CHANGELOG.md`; without one,
GitHub generates them from the commits.

## macOS signing

The macOS app (`com.lonje.froxel`) and its dmg are signed with Developer ID,
notarized, and stapled when these repository secrets are set. Without them the
app has an ad-hoc signature and Gatekeeper blocks the first launch. Android and
iOS have their own secrets, below. Windows, Linux, and FreeBSD builds are not
signed; see
[CODE_SIGNING.md](../CODE_SIGNING.md).

| Secret | Value |
|---|---|
| `MACOS_CERTIFICATE` | The Developer ID Application certificate and its private key as a `.p12`, base64: `base64 -i froxel.p12 \| pbcopy` |
| `MACOS_CERTIFICATE_PASSWORD` | The password the `.p12` was exported with |
| `APPLE_API_KEY` | The text of the App Store Connect API key, `AuthKey_<id>.p8` |
| `APPLE_API_KEY_ID` | That key's ID |
| `APPLE_API_ISSUER_ID` | The Issuer ID shown above the key list |

The certificate comes from Certificates, Identifiers & Profiles in the Apple
Developer account (Developer ID Application, created by the Account Holder).
The API key comes from App Store Connect, Users and Access, Integrations, Team
Keys, with the Developer role.

## Android signing

The APK and the Play App Bundle (`com.lonje.froxel`) are signed with the
keystore in these secrets; for Google Play it is the upload key. Without
them the job makes a throwaway debug key: the APK installs, but it cannot
update an install signed with another key.

| Secret | Value |
|---|---|
| `ANDROID_KEYSTORE` | The keystore, base64: `base64 -i froxel.keystore \| pbcopy` |
| `ANDROID_KEYSTORE_PASSWORD` | Its password |
| `PLAY_SERVICE_ACCOUNT_JSON` | The Google Play service account key (JSON), for `upload-stores` only |

`apksigner` is given only the store password, so the keystore holds one key
and that key has the same password:

```sh
keytool -genkeypair -keystore froxel.keystore -alias froxel -keyalg RSA \
  -keysize 4096 -validity 10000 -dname "CN=Froxel"
```

## iOS signing

With these secrets the device app is signed and the job uploads
`froxel-<version>-ios.ipa`. Without them it is
`froxel-<version>-ios-unsigned.ipa` with an ad-hoc signature, which a
sideloading tool re-signs. The simulator app is always ad-hoc.

| Secret | Value |
|---|---|
| `IOS_CERTIFICATE` | An Apple Development or Apple Distribution certificate and its private key as a `.p12`, base64 |
| `IOS_CERTIFICATE_PASSWORD` | The password the `.p12` was exported with |
| `IOS_PROVISIONING_PROFILE` | A profile for `com.lonje.froxel` made with that certificate, base64: `base64 -i Froxel.mobileprovision \| pbcopy` |

A Development or Ad Hoc profile installs on the devices it lists. An App
Store profile with an Apple Distribution certificate makes the TestFlight and
App Store build; that ipa goes to the `store-<version>-ios` artifact, and
`upload-stores` sends it to App Store Connect with the `APPLE_API_KEY`
secrets above. The Developer ID certificate used for macOS does not sign iOS
apps.

## Store builds

Both stores want a build number that grows with each upload.
`packaging/build_number.sh` derives it from the version and the workflow uses
it for the iOS `CFBundleVersion` and the Android `versionCode`:
`major * 1000000 + minor * 10000 + patch * 100 + stage`, where the stage is
the last number of a prerelease (1 to 98) and 99 for a release. `0.2.0-rc.1`
is 20001 and `0.2.0` is 20099. So every store upload needs a new version;
number prereleases across their kinds (`alpha.1`, `beta.2`, `rc.3`), keep
minor and patch below 100, and end each prerelease in a number. The `Version`
job fails early on a version without a build number.

iOS: the `ios` job runs on `macos-26`, since App Store Connect takes only
builds made with Xcode 26 and the iOS 26 SDK or later. The app carries
`packaging/ios/PrivacyInfo.xcprivacy`: no tracking, no collected data, and
the required-reason APIs the binary links (file timestamps, `C617.1`; system
boot time, `35F9.1`). After a dependency update, check the list again:

```sh
nm -u target/aarch64-apple-ios/release/froxel | grep -E '_(f|l)?stat$|_mach_absolute_time|_statfs|_getattrlist'
```

Android: Play takes only App Bundles. `packaging/android/bundle.sh` turns
cargo-apk's output into `target/release/apk/froxel.aab` (aapt2 in proto
format, bundletool, jarsigner; it needs a JDK). For Play, `ANDROID_KEYSTORE`
is the upload key and Play App Signing holds the app signing key. Play
requires `targetSdkVersion` 36 and 16 KB-aligned native libraries; check the
second with `llvm-readelf -lW target/release/apk/lib/arm64-v8a/libfroxel.so`
(alignment `0x4000`).

With `upload-stores`, the `ios` job validates the ipa and uploads it to
App Store Connect with `xcrun altool` and the `APPLE_API_KEY` secrets; it
needs an Apple Distribution certificate and an App Store profile. The
`android` job sends the bundle to the internal track as a draft with
`tools/play_upload.py` and `PLAY_SERVICE_ACCOUNT_JSON`. Play's API updates
only an app that exists, so the first bundle is uploaded by hand in Play
Console, and it accepts only drafts until the app has passed its first
review.

Then publish the draft on the Releases page. Publishing creates the tag.
