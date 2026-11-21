# Releasing

Before a release, move the `Unreleased` entries in `CHANGELOG.md` under the
new version and set `version` in `Cargo.toml`. Then run

```sh
python3 tools/sync_versions.py
```

which copies that version into the Godot `plugin.cfg`, the Blender
`blender_manifest.toml` and `bl_info`, `server.json`, the Claude Code
`.claude-plugin/plugin.json`, and the `Dockerfile`'s default
`FROXEL_VERSION`. The `addons` job runs it with `--check` and fails when one
is behind.

For Android, also set `version_code` under `[package.metadata.store]` in
`Cargo.toml` to `packaging/build_number.sh <version>`, and for a release
(not a prerelease) write its What's new for F-Droid, 500 characters at
most, to `fastlane/metadata/android/en-US/changelogs/<version_code>.txt`.
The `Version` job fails without them ([F-Droid](#f-droid)).

`.github/workflows/release.yml` builds every platform. It runs only on request:

```sh
gh workflow run release.yml                      # version from Cargo.toml
gh workflow run release.yml -f version=0.1.0-rc.1
gh workflow run release.yml -f draft-release=false  # artifacts only
gh workflow run release.yml -f upload-stores=true   # also App Store Connect and Google Play
```

It builds macOS universal (`.dmg`, `.zip`), Windows x64, x86 and arm64
(`.zip`), Linux x86_64, aarch64 and riscv64 (`.tar.gz`, `.deb`, `.rpm`) and FreeBSD
x86_64 (`.tar.gz`), Android arm64 (`.apk`) and iOS (`.ipa` for devices, a
zipped `Froxel.app` for the simulator), then creates or updates the draft release `v<version>`
with a `SHA256SUMS.txt`. It also zips the Godot plugin (`godot-addon.zip`,
and `godot-assetlib.zip` for the Asset Library) and builds the Blender
extension (`blender-addon.zip`) with Blender 4.2 LTS.
The Android App Bundle, an App Store-signed ipa, and the Mac App Store pkg are
workflow artifacts named `store-<version>-android`, `store-<version>-ios`, and
`store-<version>-macos`, not release files; see [Store builds](#store-builds).
The Microsoft Store bundle is `store-<version>-windows`; see
[Windows channels](#windows-channels).
The Linux x86_64 and aarch64 builds run in an AlmaLinux 8 container, so they
need only glibc 2.28; riscv64 is cross-compiled on Ubuntu 24.04 (glibc 2.39).
`packaging/linux/package.sh` makes the three Linux files from a built binary
and reads the minimum glibc for the package dependencies from it.
The release notes are the version's section of `CHANGELOG.md`; without one,
GitHub generates them from the commits.

For a release (not a prerelease), also add
`<release version="<version>" date="YYYY-MM-DD">` at the top of `<releases>` in
`packaging/linux/com.lonje.Froxel.metainfo.xml`: Flathub, GNOME Software, and
KDE Discover read it, and the `Version` job fails without it
([Linux channels](#linux-channels)).

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

## Mac App Store

Set up on 2026-11-20 through the App Store Connect API: the Mac Installer
Distribution certificate `44N4TNRZPJ` (expires 2027-11-20), the profile
`H79LZWF5F8` "Froxel Mac App Store" with the Apple Distribution certificate
`39LYAR27BK`, both identities in `MAS_CERTIFICATE` (and
`secrets/macos/mas-certificates.p12`), and the macOS version 0.3.4 with its
description, two 1440x900 screenshots, and review notes. The first run built
and signed the pkg; its upload waits for a run with the fixed launch check
(a store-signed app is killed when it is started before Apple re-signs it,
so the job checks it with `codesign --verify` instead).

The `Mac App Store` job builds a second, sandboxed `Froxel.app` on `macos-26`
with `packaging/macos/appstore.sh`: the same universal binary and bundle id
`com.lonje.froxel`, the App Sandbox entitlements in
`packaging/macos/Froxel.entitlements` (user-selected files read-write,
app-scoped bookmarks, network server and client for the loopback `--control`
port and the bridge), the profile's application and team identifiers,
`CFBundleVersion` from `packaging/build_number.sh`,
`LSApplicationCategoryType` graphics-design, `ITSAppUsesNonExemptEncryption`
false, an asset catalog icon, and the privacy manifest. `productbuild` wraps
it in `froxel-<version>-macos.pkg`, signed with the installer identity, the
`store-<version>-macos` artifact. Without the secrets the app is ad-hoc signed,
still sandboxed, and checked with `--help`; no pkg is made and the artifact is
empty. The Developer ID dmg is not sandboxed and does not change.

| Secret | Value |
|---|---|
| `MAS_CERTIFICATE` | One `.p12` with two identities and their private keys, base64: an Apple Distribution (or 3rd Party Mac Developer Application) and a Mac Installer Distribution (or 3rd Party Mac Developer Installer) certificate. Select both in Keychain Access and export them together. |
| `MAS_CERTIFICATE_PASSWORD` | The password the `.p12` was exported with |
| `MAS_PROVISIONING_PROFILE` | A Mac App Store Connect profile for `com.lonje.froxel` made with that Apple Distribution certificate, base64: `base64 -i Froxel.provisionprofile \| pbcopy` |

The iOS `IOS_CERTIFICATE` may hold the same Apple Distribution certificate;
the Developer ID certificate signs neither.

With `upload-stores`, the `Mac App Store upload` job validates and uploads the
pkg with `xcrun altool` and the `APPLE_API_KEY` secrets, like the iOS ipa. With
`submit-review`, it runs `tools/asc_release.py --platform MAC_OS` with the
`ASC_RELEASE_KEY` secrets ([Store builds](#store-builds)): the macOS version
of the same app record is edited and submitted on its own, so a macOS
rejection does not hold back iOS. iOS and macOS builds of one version share the
build number; App Store Connect keeps them apart by platform.

One-time setup:

1. Certificates, Identifiers & Profiles: in the explicit App ID
   `com.lonje.froxel`, keep the capabilities the iOS app has; the sandbox
   needs no capability there.
2. Certificates: create a Mac Installer Distribution certificate (and an
   Apple Distribution one if the iOS one is not to be shared). Install both,
   then export them together as the `MAS_CERTIFICATE` `.p12`.
3. Profiles: create a Mac App Store Connect profile (Distribution, macOS) for
   `com.lonje.froxel` with the Apple Distribution certificate, download it,
   and store it as `MAS_PROVISIONING_PROFILE`.
4. App Store Connect: open the existing Froxel app, Add Platform, macOS. Fill
   in the macOS version's screenshots (at least one at 1280x800 or larger),
   description, and App Privacy (already done for iOS: no data collected); in
   App Review Information, note that the MCP server and the Godot/Blender
   bridge listen on 127.0.0.1 only and that File > Grant Folder Access… is how
   MCP clients reach a folder.
5. Run `gh workflow run release.yml -f upload-stores=true` once; the first
   macOS build appears in TestFlight for Mac. The first submission for review
   can then be made with `submit-review` or by hand.

To try the sandbox locally, without any certificate:

```sh
cargo build --release
packaging/macos/appstore.sh          # target/appstore/Froxel.app, ad-hoc, sandboxed
open target/appstore/Froxel.app
target/appstore/Froxel.app/Contents/MacOS/froxel mcp
```

The app's container is `~/Library/Containers/com.lonje.froxel/Data`. macOS
ties a container to the signature that created it, so delete it after local
tests (`rm -rf ~/Library/Containers/com.lonje.froxel`) before installing the
store build.

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

iOS: the `ios` and `App Store` jobs run on `macos-26`, since App Store Connect takes only
builds made with Xcode 26 and the iOS 26 SDK or later; so do the two Mac
App Store jobs. The app carries
`packaging/ios/PrivacyInfo.xcprivacy`: no tracking, no collected data, and
the required-reason APIs the binary links (file timestamps, `C617.1`; system
boot time, `35F9.1`). After a dependency update, check the list again:

```sh
nm -u target/aarch64-apple-ios/release/froxel | grep -E '_(f|l)?stat$|_mach_absolute_time|_statfs|_getattrlist'
```

Android: cargo-apk sets the APK's versionCode from the version alone
(`1 << 24 | major << 16 | minor << 8 | patch`), so a prerelease and its
release share one. `packaging/android/apk.sh` links cargo-apk's manifest and
resources again with the build number above, adds the native library
uncompressed and 16 KB-aligned, and signs the result. That APK is the
release download and what F-Droid, the Amazon Appstore, and the Galaxy
Store get; it is also `froxel-<version>-android.apk` in the
`store-<version>-android` artifact. The `android` job pins Rust 1.100.0
and NDK 29.0.14206865, as the F-Droid recipe does.

Android: Play takes only App Bundles. `packaging/android/bundle.sh` turns
cargo-apk's output into `target/release/apk/froxel.aab` (aapt2 in proto
format, bundletool, jarsigner; it needs a JDK). For Play, `ANDROID_KEYSTORE`
is the upload key and Play App Signing holds the app signing key. Play
requires `targetSdkVersion` 36 and 16 KB-aligned native libraries; check the
second with `llvm-readelf -lW target/release/apk/lib/arm64-v8a/libfroxel.so`
(alignment `0x4000`).

With `upload-stores`, two more jobs take the builds' `store-<version>-*`
artifacts (`Mac App Store upload` and the other stores have sections of their
own). `App Store` validates the ipa and uploads it to App Store Connect
with `xcrun altool` and the `APPLE_API_KEY` secrets; the ipa needs an Apple
Distribution certificate and an App Store profile. `Google Play` sends the
bundle to the internal track as a draft with `tools/play_upload.py` and
`PLAY_SERVICE_ACCOUNT_JSON`. They are separate from the builds, so a store
failure fails only its own job: the artifacts and the draft GitHub Release
are made anyway. Play's API updates
only an app that exists, so the first bundle is uploaded by hand in Play
Console, and it accepts only drafts until the app has passed its first
review.

`submit-review` (with `upload-stores`, release versions only) also sends the
build for review in both stores:

```sh
gh workflow run release.yml -f upload-stores=true -f submit-review=true
```

`Google Play` then releases the bundle on the internal and production
tracks as complete, which sends production for review. `App Store` runs
`tools/asc_release.py`: it waits until App Store Connect has processed the
build, reuses the editable iOS version or creates one, sets its version and
What's New, attaches the build, and submits it. A version left in an open
review submission, including one App Review answered with unresolved issues,
counts as editable: the new build replaces the old one in that submission,
which is sent again. A reply App Review asked for still goes in App Store
Connect by hand. The version's release type
(manual or after approval) is kept. Both stores take their release notes from
the version's `CHANGELOG.md` section through `tools/release_notes.py`; without
one, Play gets none and the App Store gets "Bug fixes and improvements." A
version still in review or waiting for release stops the iOS step; finish it
first.

| Secret | Value |
|---|---|
| `ASC_RELEASE_KEY` | The text of an App Store Connect API key with the App Manager role, `AuthKey_<id>.p8` |
| `ASC_RELEASE_KEY_ID` | That key's ID |

The issuer is `APPLE_API_ISSUER_ID`. A Developer key, like the notarization
key, uploads builds but cannot submit them.

Then publish the draft on the Releases page. Publishing creates the tag.

## Android stores

Besides Google Play, Froxel's APK goes to F-Droid, the Amazon Appstore, and
the Samsung Galaxy Store. All of them get the APK from
`packaging/android/apk.sh` (see [Store builds](#store-builds)); none of them
has been run with a store account yet.

### F-Droid

F-Droid builds the app from source and signs it with its own key, so its
install does not update one from the other channels. The listing is in this
repository, in the Fastlane layout that F-Droid reads:
`fastlane/metadata/android/en-US/` holds `title.txt`, `short_description.txt`
(80 characters), `full_description.txt` (4000), `images/icon.png`,
`images/featureGraphic.png`, `images/phoneScreenshots/`,
`images/tenInchScreenshots/` (copies of `promo/store/play-*.png`), and
`changelogs/<versionCode>.txt` (500 characters), the What's new of each
release. The `Version` job fails a release without that file, or with
`version_code` in `Cargo.toml` behind the version.

The build recipe is `packaging/android/fdroid/com.lonje.froxel.yml`. It is
submitted once, as `metadata/com.lonje.froxel.yml` in a merge request to
[fdroiddata](https://gitlab.com/fdroid/fdroiddata), and from then on F-Droid
finds new releases itself: `UpdateCheckMode: Tags` takes release tags
(`v1.2.3`, not prereleases), reads `version` and `version_code` from that
tag's `Cargo.toml`, and `AutoUpdateMode: Version` adds the build. The recipe
installs Rust 1.100.0 with the `rustup` srclib, builds the pinned cargo-apk,
builds with NDK 29.0.14206865, and runs `apk.sh` without a key, which leaves
the APK unsigned for F-Droid to sign. Change the toolchain and the NDK there
and in the `android` job together.

The first recipe names the first release tag that has `apk.sh` and
`version_code` (written as `v0.4.0`, `40099`); correct its `versionName`,
`versionCode`, `commit`, and the `Current*` fields to the real one before the
merge request. To try it, with a checkout of fdroiddata next to this one:

```sh
cp packaging/android/fdroid/com.lonje.froxel.yml ../fdroiddata/metadata/
cd ../fdroiddata
docker run --rm -v "$PWD:/repo" registry.gitlab.com/fdroid/fdroidserver:buildserver \
  fdroid lint com.lonje.froxel
docker run --rm -v "$PWD:/repo" registry.gitlab.com/fdroid/fdroidserver:buildserver \
  fdroid build -v -l com.lonje.froxel
```

Status: the recipe for 0.3.4 is pushed to the `froxel` branch of the fork
`theromis/fdroiddata`, and the merge request is not open yet. F-Droid's
inclusion policy asks for an app developed for at least three months, and
Froxel's first commit is 2026-10-24, so the merge request waits until after
2027-01-24. Before opening it, rebase the branch on fdroiddata, point the
recipe at the latest release, and say in the description that Froxel and the
recipe were written with AI assistants.

The dependency tree for Android has no prebuilt binaries and no non-free
code (no Google Play Services; wgpu, winit, egui, and the `vendor/` patches
are all source). The only binary in the tree, the test fixture
`tests/fixtures/mesh/quad.bin`, is removed by the recipe before F-Droid's
scanner runs. The app asks for no permissions, so it carries no
anti-features.

### Amazon Appstore

The `Amazon Appstore` job (`upload-stores`) runs `tools/amazon_upload.py`
with the App Submission API: it reuses the app's open edit or opens one,
replaces its APK (keeping the device targeting), sets the en-US "What's new"
from `tools/release_notes.py`, and validates the edit. The edit stays a
draft in the Developer Console; with `submit-review` it is committed, which
sends the version for review. Amazon's API takes APKs, not App Bundles, and
updates only an app that exists: create the app and submit its first
version by hand in the Developer Console (upload
`froxel-<version>-android.apk` from the `store-<version>-android`
artifact). An app in review cannot be changed; the job then fails with 412.

| Secret | Value |
|---|---|
| `AMAZON_CLIENT_ID` | The client ID of a security profile (Developer Console, Settings, API Access, App Submission API), `amzn1.application-oa2-client.<id>` |
| `AMAZON_CLIENT_SECRET` | That profile's client secret |
| `AMAZON_APP_ID` | The app's ID, `amzn1.devportal.mobileapp.<id>` (the app's page, Additional information) |

### Galaxy Store

The `Galaxy Store` job (`upload-stores`) runs `tools/galaxy_upload.py` with
the Content Publish API: it uploads the APK, opens an update of the app with
the release notes as New features, adds the new binary with the device
settings of the current one, and removes the older binaries from the update.
The update stays in the Registering state in Seller Portal; with
`submit-review` it is submitted for review. The API changes only an app that
is registered and for sale, so register the app (`com.lonje.froxel`, no
Google Mobile Services) and submit its first version by hand in
[Seller Portal](https://seller.samsungapps.com).

| Secret | Value |
|---|---|
| `GALAXY_SERVICE_ACCOUNT_ID` | A service account with the Publishing scope (Seller Portal, Assistance, API Service) |
| `GALAXY_PRIVATE_KEY` | The private key downloaded when the account was made (PEM text); Samsung does not keep a copy |
| `GALAXY_CONTENT_ID` | The app's content ID in Seller Portal |

Both jobs skip with a notice when their secrets are not set, and fail
without `ANDROID_KEYSTORE`: a store app must keep one signing key, and the
throwaway key changes with every run.

## itch.io and Steam

`.github/workflows/game-stores.yml` uploads a published release to itch.io
and Steam. It starts when the draft is published, or by hand for a tag that is
already published:

```sh
gh workflow run game-stores.yml -f tag=v0.4.0                 # itch.io
gh workflow run game-stores.yml -f tag=v0.4.0 -f steam=true   # also Steam
gh workflow run game-stores.yml -f tag=v0.4.0 -f steam=true -f dry-run=true
```

Both jobs download the release files, check them against `SHA256SUMS.txt`,
and unpack them. Each one notes a missing secret in the run summary and
stops without failing. `dry-run` needs no secrets: it unpacks, runs
`butler validate` on every itch.io folder and renders the Steam VDFs, and
uploads nothing. Not run yet against real accounts: the itch.io project, the
Steamworks app, and the secrets below do not exist yet.

### itch.io

`packaging/itch/push.sh` pushes one butler channel per platform, with
`--userversion` set to the release version and `--if-changed`:

| Channel | From |
|---|---|
| `osx-universal` | `froxel-<version>-macos-universal.zip` (`Froxel.app`) |
| `windows-x64`, `windows-x86`, `windows-arm64` | `froxel-<version>-windows-<arch>.zip` |
| `linux-x86_64`, `linux-aarch64` | `froxel-<version>-linux-<arch>.tar.gz` |
| `android` | `froxel-<version>-android-arm64.apk`, as a download |

Each folder gets the channel's manifest from `packaging/itch/` as
`.itch.toml`, so the itch app launches `froxel.exe`, `froxel`, or
`Froxel.app` and offers the README as the manual. The x64 and x86 manifests
ask the itch app for the Visual C++ runtime (`vcredist-v14-*`); itch has
none for ARM64. Prereleases are skipped: the itch.io page lists releases
only, and testers get prereleases from the GitHub Releases page or the Steam
beta branch. The script runs locally too:

```sh
DRY_RUN=1 packaging/itch/push.sh dist/release 0.4.0   # prints the pushes
```

One-time setup:

1. On itch.io, create the project (Dashboard → Create new project), kind
   **Downloadable**, classification **Tools**, with the store text of
   `packaging/steam/store.md`. Leave it as a draft until the first push has
   landed, then set its visibility.
2. Create an API key (Settings → API keys, or `butler login` and copy
   `~/.config/itch/butler_creds`) and add it as the `BUTLER_API_KEY`
   repository secret.
3. The project is `theromis/froxel` unless the repository variable
   `ITCH_PROJECT` names another `user/game`. `BUTLER_VERSION` pins butler
   (default `LATEST`).
4. After the first push, mark the `android` upload as Android in the
   project's upload list if itch did not; the channel names tag the others.

### Steam

`packaging/steam/build.sh` unpacks Windows x64, macOS universal, and Linux
x86_64 into one depot each, fills in the SteamPipe scripts
(`packaging/steam/app_build.vdf` and `depot_*.vdf`), and runs
`steamcmd +login <user> +run_app_build`. Every `@NAME@` in the VDFs is a
placeholder; there are no real IDs in the repository. The build is set live
on `STEAM_BRANCH` (default `beta`); the script refuses `default` and
`public`, so a build reaches players only when it is set live on default by
hand in Steamworks (Builds) after testing the beta. The job runs on a
published release only when the repository variable `STEAM_ON_RELEASE` is
`true`, and by hand with `steam=true`. The steamcmd logs and the rendered
VDFs are kept as the `steam-<version>-logs` artifact.

```sh
DRY_RUN=1 packaging/steam/build.sh dist/release 0.4.0   # stages and renders
```

One-time setup:

1. Join Steamworks (steamworks.com, a Steam Direct fee of $100 per app,
   with the tax and bank forms) and create the app. Its type is
   **Software**.
2. In App Admin → SteamPipe → Depots, add three depots and set their
   operating systems: Windows (64-bit), macOS, and Linux. Steam numbers
   them after the app ID; `build.sh` assumes app ID + 1, + 2, + 3 unless
   the variables say otherwise.
3. In Installation → General, add the launch options: `froxel.exe` for
   Windows, `Froxel.app` for macOS, and `froxel` for Linux (no Steam Linux
   Runtime container is needed; it uses the system's Vulkan driver). Publish
   the changes.
4. Add a beta branch named `beta` in SteamPipe → Builds (a password is
   optional), and publish.
5. Repository variables: `STEAM_APP_ID`, and if they differ from the
   convention `STEAM_DEPOT_WINDOWS_X64`, `STEAM_DEPOT_MACOS`,
   `STEAM_DEPOT_LINUX_X86_64`; optionally `STEAM_BRANCH` and
   `STEAM_ON_RELEASE`.
6. Make a Steam account for builds only, give it the Steamworks permission
   to edit app metadata and publish builds (not to set the default branch
   live), and log it in once with steamcmd on a trusted machine so Steam
   Guard caches the login:

   ```sh
   steamcmd +login froxel_builder +quit   # asks for the password and the Steam Guard code
   base64 -i ~/Library/Application\ Support/Steam/config/config.vdf | pbcopy
   ```

   On Linux the file is `config/config.vdf` in the steamcmd folder
   (`~/Steam` or `~/.steam/steam`), and `base64 -w0` prints it.

   Add the account name as `STEAM_USERNAME` and the base64 text as
   `STEAM_CONFIG_VDF`. The workflow never sees the password. Steam drops a
   cached login after a long time unused or after a password change; then
   log in again and replace the secret.
7. Fill in the store page from `packaging/steam/store.md` and submit it
   and the build for Valve's review before the first release.

## Windows channels

Windows reaches four channels besides the release zips. Each publish step is
skipped, with a notice, until its secret is set. The Partner Center app exists
(Store ID `9NZZHD2J1JJZ`, identity `RomanVasilyev.Froxel`, set in the
`MSSTORE_*` variables) and its first submission, 0.3.4-rc.2, went to
certification by hand on 2026-11-20; the Partner Center API secrets, winget,
and Chocolatey are not set up yet.

| Channel | Made by | Published by | Prereleases |
|---|---|---|---|
| Microsoft Store | `release.yml`, jobs `windows` and `Microsoft Store bundle` | `release.yml` job `Microsoft Store`, with `upload-stores` | Only to the package flight `MSSTORE_FLIGHT_ID`; skipped without one |
| winget | `windows-repos.yml` from the release zips | a pull request to `microsoft/winget-pkgs` | Skipped: winget has no prerelease track |
| Scoop | `windows-repos.yml` | a commit of `bucket/froxel.json` to `main` | Skipped: the bucket follows releases |
| Chocolatey | `windows-repos.yml` | `choco push` to the community repository | Pushed as a prerelease (`0.4.0-rc.1` is `0.4.0-rc01`) |

`.github/workflows/windows-repos.yml` runs when a release is published. By
hand it takes a tag and is a dry run unless `dry-run` is off; the dry run
writes and checks every manifest, installs from each (winget when the runner
has it, Scoop, Chocolatey), builds the MSIX from the release's x64 zip,
installs a test-signed copy, and publishes nothing:

```sh
gh workflow run windows-repos.yml -f tag=v0.3.3                  # dry run
gh workflow run windows-repos.yml -f tag=v0.3.3 -f dry-run=false # publish again
```

`packaging/windows/channels.py <version> SHA256SUMS.txt` writes the winget,
Scoop, and Chocolatey files from the release checksums, and
`packaging/windows/check_manifests.py` checks the winget and Scoop ones
against their JSON schemas on any system.

### Microsoft Store

The `windows` job packs each build into an unsigned MSIX
(`packaging/windows/msix.ps1`: `AppxManifest.xml`, the logos in
`packaging/windows/Assets`, `makepri` and `makeappx` from the Windows SDK) and
installs a test-signed copy of the x64 one (`msix_check.ps1`). `Microsoft
Store bundle` joins the three into `froxel-<version>-windows.msixbundle` and
the `.msixupload` Partner Center takes, the `store-<version>-windows`
artifact. The package is a full-trust desktop app (`runFullTrust`), declares
`internetClientServer` and `privateNetworkClientServer` for the loopback
`--control` and bridge ports, opens `.vox` files, and puts `froxel` on the
PATH through an execution alias (console subsystem, so `froxel mcp` keeps
stdio). The MSIX version is `major.minor.(patch * 100 + stage).0` from
`packaging/build_number.sh`: `0.3.3` is `0.3.399.0`, `0.4.0-rc.1` is
`0.4.1.0`, and `0.4.0` is `0.4.99.0`, so every upload is newer than the last. A store package failure does not hold
back the zips or the draft release.

The logos come from `packaging/icons/froxel.png`; after `make_icons.py`, run
`uv run --with pillow python packaging/windows/make_logos.py`.

One-time setup:

1. Register as an app developer in
   [Partner Center](https://partner.microsoft.com/dashboard) (an individual
   account is free) and reserve the name Froxel.
2. Under the app, Product management, Product identity, copy
   `Package/Identity/Name`, `Package/Identity/Publisher`, and
   `Package/Properties/PublisherDisplayName` into the repository variables
   below, and the Store ID (`9N…`) into `MSSTORE_PRODUCT_ID`.
3. Fill in the first submission (pricing, properties, age rating, store
   listing, and the `runFullTrust` justification: Froxel is a desktop editor
   that reads and writes the user's model files and runs a local MCP server)
   and upload the first `.msixupload` from the `store-<version>-windows`
   artifact by hand.
4. In Partner Center, Account settings, User management, Microsoft Entra
   applications, add an Entra ID (Azure AD) app with the Manager role, and
   create a client secret for it. Its tenant ID, client ID, and secret, and
   the Seller ID from Account settings, Legal info, are the secrets below.
5. For prereleases, create a package flight under the app and set its ID as
   `MSSTORE_FLIGHT_ID`.

| Repository variable | Value |
|---|---|
| `MSSTORE_IDENTITY_NAME` | `Package/Identity/Name`, as in `12345Lonje.Froxel` |
| `MSSTORE_PUBLISHER` | `Package/Identity/Publisher`, as in `CN=…` |
| `MSSTORE_PUBLISHER_DISPLAY_NAME` | `Package/Properties/PublisherDisplayName` |
| `MSSTORE_PRODUCT_ID` | The Store ID of the app |
| `MSSTORE_FLIGHT_ID` | Optional: the package flight for prereleases |

| Secret | Value |
|---|---|
| `PARTNER_CENTER_TENANT_ID` | The Entra ID tenant |
| `PARTNER_CENTER_SELLER_ID` | The Partner Center Seller ID |
| `PARTNER_CENTER_CLIENT_ID` | The Entra ID app's client (application) ID |
| `PARTNER_CENTER_CLIENT_SECRET` | Its client secret; it expires, so renew it before then |

Without the variables the MSIX carries a placeholder identity: it installs
test-signed, and Partner Center refuses it. With `upload-stores`, the
`Microsoft Store` job sends the `.msixupload` with the Microsoft Store
Developer CLI (`msstore publish --noCommit`), which leaves a draft
submission; with `submit-review` it commits the submission, which sends it
to certification. `msstore publish` replaces a pending draft, so edit the
listing after the upload, not before.

### winget

The manifests are `Lonje.Froxel`, schema 1.10: the release zip of each
architecture with the nested portable `froxel.exe` and the `froxel` command.
The first version is reviewed by the winget maintainers like any other;
later ones are merged by their bots once validation passes.

1. Make a classic personal access token with the `public_repo` scope on an
   account that may fork `microsoft/winget-pkgs` (wingetcreate forks it and
   opens the pull request from the fork).
2. Save it as the `WINGET_TOKEN` secret.

### Scoop

The bucket is this repository's `bucket/` folder, not a repository of its
own: Scoop reads manifests from a `bucket/` folder of any repository, so
there is no second repository to keep or token to store, and the manifest is
reviewed with the code. `windows-repos.yml` commits the new
`bucket/froxel.json` to `main` with `GITHUB_TOKEN` when a release is
published. The manifest also has `checkver` and `autoupdate` (from the
release's `SHA256SUMS.txt`), so `scoop`'s own `checkver.ps1 -u` updates it
by hand too, and it can move to the Extras bucket unchanged if Froxel gets
in there. Nothing to set up; a branch protection rule on `main` would have
to let `github-actions[bot]` push.

### Chocolatey

`packaging/windows/chocolatey` is the package: `froxel.nuspec` and
`tools/chocolateyinstall.ps1`, which downloads the release zip (x86, x64,
or arm64 on an ARM64 Windows) and checks its SHA-256; Chocolatey shims
`froxel.exe` onto the PATH. `channels.py` fills in the version and checksums.

1. Make an account on [community.chocolatey.org](https://community.chocolatey.org).
2. Copy the API key from the account page into the `CHOCOLATEY_API_KEY`
   secret.

Every version waits in the community moderation queue (automatic checks,
then a human for new packages) before `choco install froxel` finds it.

## Registries

Publishing the draft release starts `.github/workflows/mcp-registry.yml`: it
pushes the `ghcr.io/theromis/froxel-mcp` image and publishes `server.json` to
the official MCP Registry with GitHub OIDC; pre-releases are skipped. The
Claude Code plugin is read from `main`, so it needs nothing beyond the version
bump. Then, by hand:

- Godot Asset Library:
  `python3 tools/godot_assetlib.py --asset-id <id> --version <version>`
  (with `GODOT_ASSETLIB_USER` and `GODOT_ASSETLIB_PASSWORD`), then wait for a
  moderator.

The first listings, the web forms, and what each registry needs are in
[registries.md](registries.md).

## Linux channels

Publishing the draft release starts `.github/workflows/linux-repos.yml`
(also `gh workflow run linux-repos.yml -f tag=v<version>`). It downloads the
release's Linux files, checks them against `SHA256SUMS.txt`, and sends them to
every channel whose secret is set; a channel without its secret is skipped.
Prereleases go only to the Snap Store's `beta` channel.

| Channel | Users run | Secrets and variables | Files |
|---|---|---|---|
| apt on GitHub Pages | the commands on `https://theromis.github.io/Froxel/` | `APT_GPG_PRIVATE_KEY`, optional `APT_GPG_PASSPHRASE` | `packaging/linux/apt` |
| COPR | `sudo dnf copr enable theromis/froxel && sudo dnf install froxel` | `COPR_CONFIG`; variable `COPR_PROJECT` (default `theromis/froxel`) | `packaging/linux/copr` |
| openSUSE Build Service | `zypper addrepo` of the project's repository | `OBS_TOKEN`, `OBS_USER`; variable `OBS_PROJECT` (default `home:<user>`) | `packaging/linux/copr` (the same spec), `packaging/linux/obs/_service` |
| AUR | `yay -S froxel-bin` | `AUR_SSH_KEY`; variable `PACKAGE_MAINTAINER` | `packaging/linux/aur` |
| Snap Store | `sudo snap install froxel` | `SNAPCRAFT_STORE_CREDENTIALS` | `packaging/snap` |
| Flathub | `flatpak install flathub com.lonje.Froxel` | `FLATHUB_TOKEN` | `packaging/flatpak` |
| Homebrew tap | `brew install --cask theromis/froxel/froxel` (macOS), `brew install theromis/froxel/froxel` (Linux) | `HOMEBREW_TAP_DEPLOY_KEY`; variable `HOMEBREW_TAP` (default `<owner>/homebrew-froxel`) | `packaging/homebrew` |
| Gentoo GURU, Void, FreeBSD ports, nixpkgs | by hand, from the `recipes-<version>` artifact | variables `PACKAGE_MAINTAINER` (`Name <email>`), `PACKAGE_MAINTAINER_NAME`, `PACKAGE_MAINTAINER_EMAIL` | `packaging/{gentoo,void,freebsd,nix}` |
| Nix flake | `nix run github:theromis/Froxel` | none, it builds `main` or a tag | `flake.nix`, `packaging/nix` |

Set up on 2026-11-20: the apt repository (key in `secrets/linux`), COPR
`theromis/froxel` (Fedora 43–45 and rawhide, EPEL 8–10; the `COPR_CONFIG`
token expires on 2027-05-19, renew it from <https://copr.fedorainfracloud.org/api/>),
OBS `home:theromis/froxel` (Tumbleweed x86_64, Tumbleweed ARM as
`openSUSE_Factory_ARM`, Leap 16.0 and 15.6 for x86_64 and aarch64; token
13232 in `OBS_TOKEN` and `secrets/linux/obs-token`; the package holds only the
`_service` and must not have `scmsync` in its meta), and the Homebrew tap. Not yet: the Snap name
(`snapcraft register froxel`, then `export-login`), the AUR (registration is
closed), and Flathub (needs a release with the metainfo).

The AUR, COPR, OBS, Gentoo, and Homebrew packages repack the release
tarballs, which carry the binary, `froxel.desktop`, the AppStream metadata,
and the hicolor icons. Flathub, Void, FreeBSD, and Nix build from source.

### apt repository

1. Make a signing key with no expiry you will forget about, for example
   `gpg --quick-gen-key "Froxel apt <noreply@github.com>" ed25519 sign 5y`,
   and store `gpg --armor --export-secret-keys <fingerprint>` in
   `APT_GPG_PRIVATE_KEY`.
2. Settings → Pages → Source: **GitHub Actions**.
3. Settings → Environments → github-pages: allow the tag pattern `v*` beside
   `main`, or the run a published release starts cannot deploy (done).

Each publish replaces the site with the latest release; `froxel.gpg` there is
the public key users install. A new key needs the users to fetch it again.

### COPR

1. Sign in at [copr.fedorainfracloud.org](https://copr.fedorainfracloud.org)
   with a Fedora account and create the project `froxel` with the chroots
   `fedora-*-x86_64`, `fedora-*-aarch64`, `epel-8-*`, `epel-9-*`, and
   `epel-10-*`.
2. Copy the API text from <https://copr.fedorainfracloud.org/api/> into
   `COPR_CONFIG`.

The source rpm carries the three tarballs, so the build needs no network
access.

### openSUSE Build Service

1. Make an account on [build.opensuse.org](https://build.opensuse.org),
   create the package `froxel` in `home:<user>` (or the `OBS_PROJECT`), and add
   the repositories `openSUSE_Tumbleweed` and `15.6` for x86_64 and aarch64.
2. Add `packaging/linux/obs/_service` to the package as its only file (the
   web editor, or `osc add`); it fetches `froxel.spec` from the latest
   release and the tarballs that the spec's Source lines name.
3. Profile, Tokens: create a token of type `service` for the package and put
   it in `OBS_TOKEN`; set `OBS_USER` (it only names the default project).

The job attaches the filled-in `froxel.spec` to the release, then triggers
the service with the token (`/trigger/runservice`), so no account password is
stored. OBS takes only Basic auth from `osc`, not SSH keys, which is why the
upload goes through the service. The service always reads the latest
release, so a rerun for an older tag rebuilds the latest one.

### AUR

1. Make an account on [aur.archlinux.org](https://aur.archlinux.org) and add
   an SSH public key to it; put the private key in `AUR_SSH_KEY`.
2. Set the variable `PACKAGE_MAINTAINER` to `Name <email>`.

The first push creates `froxel-bin`. `packaging/linux/aur/generate.sh` writes
the PKGBUILD and the `.SRCINFO` that `makepkg --printsrcinfo` would print.

### Snap Store

1. `snapcraft register froxel`, then
   `snapcraft export-login --snaps froxel --channels stable,beta -` into
   `SNAPCRAFT_STORE_CREDENTIALS`.

The snap repacks the tarball on core24 with the GNOME extension (Mesa and
Vulkan come from it), strictly confined with `home`, `removable-media`,
`network`, and `network-bind`. Every run builds the amd64 and arm64 snaps as
`snap-<arch>` artifacts, with or without the secret.

### Flathub

The app id is `com.lonje.Froxel`. The first submission is by hand:

1. Put the verification token Flathub shows into
   `https://lonje.com/.well-known/org.flathub.VerifiedApps.txt`.
2. `packaging/flatpak/flathub.sh v<version> <dir>` writes the manifest for the
   tag and its `cargo-sources.json`. Open a pull request with both against
   the `new-pr` branch of [flathub/flathub](https://github.com/flathub/flathub),
   following its submission guide.
3. Once it is accepted, Flathub makes `flathub/com.lonje.Froxel`. A token with
   push access to it in `FLATHUB_TOKEN` lets later releases open the update
   pull request; without it, the `flathub-<version>` artifact holds the files.

The tag has to contain `packaging/linux/com.lonje.Froxel.metainfo.xml`, so
v0.3.3 and older cannot be submitted. To build locally:
`packaging/flatpak/flathub.sh --local`, then
`flatpak-builder --user --install --force-clean build packaging/flatpak/com.lonje.Froxel.yml`.
The sandbox has `--share=network` for the loopback MCP control port and
bridges, and `--filesystem=home` because MCP tools take file paths.

### Homebrew

1. Create the public repository `<owner>/homebrew-froxel` (done:
   `theromis/homebrew-froxel`).
2. Add a deploy key with write access to it and store the private key in
   `HOMEBREW_TAP_DEPLOY_KEY` (done; the key is in `secrets/macos`):
   `ssh-keygen -t ed25519 -f tap-key`, then
   `gh repo deploy-key add tap-key.pub -R <owner>/homebrew-froxel --allow-write`
   and `gh secret set HOMEBREW_TAP_DEPLOY_KEY < tap-key`.

The cask installs the notarized dmg and links `froxel` onto the PATH; the
formula is the Linux tarball.

### Gentoo, Void, FreeBSD, nixpkgs

These take a pull request by hand from the `recipes-<version>` artifact:

- Gentoo GURU: `gentoo/media-gfx/froxel-bin` (ebuild, thin Manifest,
  `metadata.xml`), pushed to GURU's `dev` branch once you have GURU access.
- Void: `void/srcpkgs/froxel/template`, a pull request to
  [void-linux/void-packages](https://github.com/void-linux/void-packages); test it
  there with `./xbps-src pkg froxel`.
- FreeBSD: `freebsd/graphics/froxel`; on FreeBSD run `make makesum` (writes
  `distinfo`) and `make stage check-plist package`, then file it on
  [bugs.freebsd.org](https://bugs.freebsd.org).
- nixpkgs: `nixpkgs/package.nix` goes to `pkgs/by-name/fr/froxel/package.nix`
  with `src` replaced by `fetchFromGitHub` at the tag and `cargoLock` by
  `cargoHash`; `nix-build -A froxel` prints the hash to put there.

