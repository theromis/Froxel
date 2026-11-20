# Froxel for Nix: `nix run github:theromis/Froxel`, or `nix build` in a checkout
# (flake.nix). For nixpkgs (pkgs/by-name/fr/froxel/package.nix), `src` becomes
# fetchFromGitHub at the release tag and `cargoLock` becomes `cargoHash`
# (docs/releasing.md, "Nix").
{
  lib,
  stdenv,
  rustPlatform,
  makeWrapper,
  vulkan-loader,
  wayland,
  libxkbcommon,
  libX11,
  libXcursor,
  libXi,
  libXrandr,
  fontconfig,
}:

let
  root = ../..;
  cargo = lib.importTOML (root + "/Cargo.toml");
  # Wayland, X11, Vulkan, and fontconfig are opened at run time.
  runtimeLibs = [
    vulkan-loader
    wayland
    libxkbcommon
    libX11
    libXcursor
    libXi
    libXrandr
    fontconfig
  ];
in
rustPlatform.buildRustPackage {
  pname = "froxel";
  inherit (cargo.package) version;

  src = lib.fileset.toSource {
    inherit root;
    # Everything but a local build tree.
    fileset = lib.fileset.difference root (lib.fileset.maybeMissing (root + "/target"));
  };

  cargoLock.lockFile = root + "/Cargo.lock";

  nativeBuildInputs = [ makeWrapper ];

  # The unit tests open sockets and a GPU, which the build sandbox has neither of.
  doCheck = false;

  postInstall = lib.optionalString stdenv.hostPlatform.isLinux ''
    install -Dm644 packaging/linux/froxel.desktop $out/share/applications/froxel.desktop
    install -Dm644 packaging/linux/com.lonje.Froxel.metainfo.xml $out/share/metainfo/com.lonje.Froxel.metainfo.xml
    for png in packaging/icons/hicolor/*.png; do
      install -Dm644 "$png" "$out/share/icons/hicolor/$(basename "$png" .png)/apps/froxel.png"
    done
  '';

  postFixup = lib.optionalString stdenv.hostPlatform.isLinux ''
    wrapProgram $out/bin/froxel --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
  '';

  meta = {
    description = "Voxel editor with a path tracer and an MCP server";
    homepage = "https://github.com/theromis/Froxel";
    changelog = "https://github.com/theromis/Froxel/blob/main/CHANGELOG.md";
    license = lib.licenses.mit;
    mainProgram = "froxel";
    platforms = lib.platforms.linux ++ lib.platforms.darwin;
  };
}
