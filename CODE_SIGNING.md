# Code signing policy

macOS builds are signed with the maintainer's Apple Developer ID and notarized
by Apple. The Mac App Store build is the same source in the App Sandbox,
signed with the maintainer's Apple Distribution certificate and a Mac App
Store provisioning profile, and packaged with the Mac Installer Distribution
certificate; Apple re-signs it for the store.

Windows builds are not code-signed. SmartScreen may ask before the first
launch (**More info → Run anyway**), and Windows 11 with Smart App Control
turned on blocks unsigned programs outright. Signing will come back when the
project qualifies for a free open source certificate or takes up a paid one.
The Microsoft Store build is the exception: the release workflow makes an
unsigned MSIX and the Store signs it with Microsoft's certificate when it
publishes it. winget, Scoop, and Chocolatey install the same unsigned zip as
the release, checked against its SHA-256.

Linux and FreeBSD builds are not signed; check them against `SHA256SUMS.txt`.

## What gets signed

Only files built from this repository's source by the
[Release workflow](.github/workflows/release.yml) on GitHub Actions. Nothing
is signed on a personal machine, and no third-party binaries are signed. Each
release lists the SHA-256 of every file in `SHA256SUMS.txt`.

## Team

- Committers and reviewers: [Roman Vasilyev](https://github.com/theromis)
- Release approvers: [Roman Vasilyev](https://github.com/theromis)

Changes from other contributors arrive as pull requests and are reviewed by a
committer before they are merged. Releases are started by hand and published
only after an approver checks the draft.

## Privacy

Froxel sends no telemetry and no personal data. It opens network connections
only when you ask for them: the `--control` and `--bridge` ports listen on
`127.0.0.1`, and `froxel mcp --connect` connects to the address you pass it.
The Mac App Store build additionally runs in the App Sandbox: it reads and
writes only its container, files you open or save, and folders you grant with
File > Grant Folder Access….
