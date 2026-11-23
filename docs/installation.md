# Installing Froxel

Every release is on the
[releases page](https://github.com/theromis/Froxel/releases/latest) as plain
files, with a `SHA256SUMS.txt`. The package managers and stores below install
the same build and keep it updated.

Froxel is not in the main repositories of any Linux distribution or BSD yet:
`apt install froxel` or `dnf install froxel` finds it only after you add
Froxel's own repository, as shown for each system below. Requests to include
it in FreeBSD ports, Flathub, the AUR, Void, and nixpkgs are under way; until
they land, use a repository below or the files from the release.

- [macOS](#macos)
- [Windows](#windows)
- [Linux](#linux): [Debian and Ubuntu](#debian-ubuntu-mint-pop_os),
  [Fedora and RHEL](#fedora-rhel-centos-stream-rocky-alma),
  [openSUSE](#opensuse), [Arch](#arch-manjaro), [Snap](#snap),
  [Homebrew](#homebrew-on-linux), [Nix](#nixos-and-nix),
  [any other](#any-other-linux)
- [FreeBSD](#freebsd)
- [Android](#android)
- [iOS](#ios)
- [Godot and Blender add-ons](#godot-and-blender-add-ons)
- [From source](#from-source)

## macOS

macOS 11 or newer, Apple silicon and Intel.

**Homebrew** (updates with `brew upgrade`):

```sh
brew install --cask theromis/froxel/froxel
```

The cask installs the app into Applications and puts `froxel` on the PATH.

**Disk image:** download `froxel-<version>-macos-universal.dmg` and drag Froxel
to Applications. It is signed with a Developer ID and notarized. To use the
command line, link the binary:

```sh
sudo ln -s /Applications/Froxel.app/Contents/MacOS/froxel /usr/local/bin/froxel
```

**Mac App Store:** in review. That build runs in the App Sandbox: its MCP
tools reach only folders granted with **File > Grant Folder Access…**, and it
has no `--control` port and no Godot/Blender bridge
([details](mcp.md#mac-app-store-build)). Use Homebrew or the disk image for
those.

## Windows

Windows 10 1809 or newer, x64, x86, and ARM64.

**Microsoft Store:**
[Froxel on the Microsoft Store](https://apps.microsoft.com/detail/9NZZHD2J1JJZ).
The Store signs the package, so SmartScreen does not ask; `froxel` works in a
terminal and `.vox` files open in the app.

**Scoop** (updates with `scoop update froxel`):

```powershell
scoop bucket add froxel https://github.com/theromis/Froxel
scoop install froxel
```

**winget:** in review. Once it is accepted:

```powershell
winget install Lonje.Froxel
```

**Chocolatey:** in moderation. Once it is approved:

```powershell
choco install froxel
```

**Zip:** download `froxel-<version>-windows-<arch>.zip`, unpack it, and run
`froxel.exe`. The zip is not code-signed yet ([why](../CODE_SIGNING.md)), so
SmartScreen may ask once (**More info → Run anyway**), and Windows 11 with
Smart App Control on blocks it. The Microsoft Store and Scoop avoid both.

## Linux

x86_64, ARM64 (aarch64), and RISC-V (riscv64), glibc 2.28 or newer (2.39 for
riscv64). Froxel draws with Vulkan, so it needs a Vulkan driver: Mesa, or the
GPU vendor's. The packages pull in the Vulkan loader.

### Debian, Ubuntu, Mint, Pop!_OS

Froxel's apt repository, Debian 10+ and Ubuntu 20.04+ (updates with
`sudo apt upgrade`):

```sh
sudo curl -fsSLo /usr/share/keyrings/froxel.gpg https://theromis.github.io/Froxel/froxel.gpg
echo "deb [signed-by=/usr/share/keyrings/froxel.gpg] https://theromis.github.io/Froxel stable main" \
  | sudo tee /etc/apt/sources.list.d/froxel.list
sudo apt update && sudo apt install froxel
```

Or install a downloaded `.deb` once, with no updates:

```sh
sudo apt install ./froxel-*.deb
```

### Fedora, RHEL, CentOS Stream, Rocky, Alma

Froxel's COPR repository: Fedora and RHEL 8+ and its rebuilds, x86_64 and
ARM64 (updates with `sudo dnf upgrade`):

```sh
sudo dnf copr enable theromis/froxel
sudo dnf install froxel
```

`dnf copr` comes with `dnf-plugins-core`. Or install a downloaded `.rpm` once:

```sh
sudo dnf install ./froxel-*.rpm
```

### openSUSE

Froxel's repository on the openSUSE Build Service. Pick the line for your
release:

```sh
# Tumbleweed, x86_64
sudo zypper addrepo https://download.opensuse.org/repositories/home:/theromis/openSUSE_Tumbleweed/home:theromis.repo
# Tumbleweed, aarch64
sudo zypper addrepo https://download.opensuse.org/repositories/home:/theromis/openSUSE_Factory_ARM/home:theromis.repo
# Leap 16.0
sudo zypper addrepo https://download.opensuse.org/repositories/home:/theromis/16.0/home:theromis.repo
# Leap 15.6
sudo zypper addrepo https://download.opensuse.org/repositories/home:/theromis/15.6/home:theromis.repo
```

then:

```sh
sudo zypper refresh && sudo zypper install froxel
```

zypper asks once to trust the repository's key.

### Arch, Manjaro

The AUR package `froxel-bin` is not published yet (AUR registration is
closed for now). Until then, use the [tarball](#any-other-linux),
[Snap](#snap), or [Homebrew](#homebrew-on-linux).

### Snap

Any distribution with snapd, x86_64 and ARM64; updates come by themselves:

```sh
sudo snap install froxel
```

The snap is strictly confined: it reads and writes files in your home folder
and on removable media.

### Homebrew on Linux

```sh
brew install theromis/froxel/froxel
```

### NixOS and Nix

Run it from the flake, which builds `main`:

```sh
nix run github:theromis/Froxel
```

or a release tag: `nix run github:theromis/Froxel/v0.3.5`. To keep it,
`nix profile install github:theromis/Froxel`.

### Any other Linux

Download `froxel-<version>-linux-<arch>.tar.gz`, unpack it, and run
`froxel`. It carries `froxel.desktop` and the icons for a menu entry.

### Flathub

Not on Flathub yet; the submission is planned. Once it is accepted:
`flatpak install flathub com.lonje.Froxel`.

## FreeBSD

FreeBSD 15.1 or newer on x86_64 (the build machine's release). Download `froxel-<version>-freebsd-x86_64.tar.gz`,
unpack it, and run `froxel`. It needs `vulkan-loader`, `libxkbcommon`, and
`wayland` (`pkg install vulkan-loader libxkbcommon wayland`). A port,
`graphics/froxel`, is in review for the ports tree
([PR 301031](https://bugs.freebsd.org/bugzilla/show_bug.cgi?id=301031)); after that it will be
`pkg install froxel`.

## Android

Android 7.0 or newer, ARM64.

**Google Play:**
[Froxel on Google Play](https://play.google.com/store/apps/details?id=com.lonje.froxel).

**APK:** download `froxel-<version>-android-arm64.apk` and open it on the
phone; Android asks once to allow installs from the browser or file manager.

F-Droid, the Amazon Appstore, and the Galaxy Store are planned. F-Droid will
build the app from source and sign it with its own key, so its install will
not update one from Google Play or the APK, and the other way round.

## iOS

iOS 16 or newer. The App Store release is in review.

`froxel-<version>-ios-simulator.zip` is the simulator build for developers:
unzip it and drag `Froxel.app` onto a booted simulator.

## Godot and Blender add-ons

- **Godot 4.3+:** unzip `froxel-<version>-godot-addon.zip` into the project
  folder, or install Froxel Bridge from the Godot Asset Library once it is
  approved.
- **Blender 4.2+:** in Blender, Preferences → Get Extensions → Install from
  Disk, and pick `froxel-<version>-blender-addon.zip`.

Both drive a running Froxel through `froxel bridge`; see
[bridges.md](bridges.md).

## From source

You need [Rust](https://rustup.rs). On Linux the editor loads Wayland or X11
and Vulkan at run time, so the build needs no `-dev` packages.

```sh
git clone https://github.com/theromis/Froxel
cd Froxel
cargo run --release -- examples/frog.vox
```

The binary is `target/release/froxel`.

## Coming

- Mac App Store and iOS App Store: in review.
- winget, Chocolatey, the Godot Asset Library: in review.
- AUR, Flathub, FreeBSD ports, Void, nixpkgs: submissions under way.
- F-Droid, the Amazon Appstore, the Galaxy Store, itch.io, and Steam: planned.
