# Releasing

Before a release, move the `Unreleased` entries in `CHANGELOG.md` under the
new version and set `version` in `Cargo.toml`.

`.github/workflows/release.yml` builds every platform. It runs only on request:

```sh
gh workflow run release.yml                      # version from Cargo.toml
gh workflow run release.yml -f version=0.1.0-rc.1
gh workflow run release.yml -f draft-release=false  # artifacts only
```

It builds macOS universal (`.dmg`, `.zip`), Windows x64, x86 and arm64
(`.zip`), Linux x86_64, aarch64 and riscv64 (`.tar.gz`, `.deb`) and FreeBSD
x86_64 (`.tar.gz`), Android arm64 (`.apk`) and iOS (`.ipa` for devices, a
zipped `Froxel.app` for the simulator), then creates or updates the draft release `v<version>`
with a `SHA256SUMS.txt`. It also zips the Godot plugin and the Blender add-on.
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

The APK (`app.froxel`) is signed with the keystore in these secrets. Without
them the job makes a throwaway debug key: the APK installs, but it cannot
update an install signed with another key.

| Secret | Value |
|---|---|
| `ANDROID_KEYSTORE` | The keystore, base64: `base64 -i froxel.keystore \| pbcopy` |
| `ANDROID_KEYSTORE_PASSWORD` | Its password |

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

A Development or Ad Hoc profile installs on the devices it lists. The
Developer ID certificate used for macOS does not sign iOS apps.

Then publish the draft on the Releases page. Publishing creates the tag.
