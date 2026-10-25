# Code signing policy

Windows builds: free code signing provided by [SignPath.io](https://about.signpath.io),
certificate by [SignPath Foundation](https://signpath.org). The application to
SignPath Foundation is under review, so releases up to and including 0.2.0 ship
unsigned Windows executables.

macOS builds are signed with the maintainer's Apple Developer ID and notarized
by Apple.

## What gets signed

Only files built from this repository's source by the
[Release workflow](.github/workflows/release.yml) on GitHub Actions. Nothing
is signed on a personal machine, and no third-party binaries are signed. Each
release lists the SHA-256 of every file in `SHA256SUMS.txt`.

## Team

- Committers and reviewers: [Roman Vasilyev](https://github.com/theromis)
- Approvers: [Roman Vasilyev](https://github.com/theromis)

Changes from other contributors arrive as pull requests and are reviewed by a
committer before they are merged. An approver authorizes each signing request.

## Privacy

Froxel sends no telemetry and no personal data. It opens network connections
only when you ask for them: the `--control` and `--bridge` ports listen on
`127.0.0.1`, and `froxel mcp --connect` connects to the address you pass it.
