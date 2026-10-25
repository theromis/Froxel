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
x86_64 (`.tar.gz`), then creates or updates the draft release `v<version>`
with a `SHA256SUMS.txt`. It also zips the Godot plugin and the Blender add-on.
The release notes are the version's section of `CHANGELOG.md`; without one,
GitHub generates them from the commits.

## macOS signing

The macOS app (`com.lonje.froxel`) and its dmg are signed with Developer ID,
notarized, and stapled when these repository secrets are set. Without them the
app has an ad-hoc signature and Gatekeeper blocks the first launch. The other
platforms are unsigned.

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

Then publish the draft on the Releases page. Publishing creates the tag.
