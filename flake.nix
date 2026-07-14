{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    systems.url = "github:nix-systems/default";

    rust-overlay.url = "github:oxalica/rust-overlay";
    crane.url = "github:ipetkov/crane";
  };

  outputs =
    inputs:
    inputs.flake-parts.lib.mkFlake { inherit inputs; } {
      systems = import inputs.systems;

      perSystem =
        {
          self',
          system,
          ...
        }:
        let
          pkgs = import inputs.nixpkgs {
            inherit system;
            overlays = [
              inputs.rust-overlay.overlays.default
            ];
          };
          lib = pkgs.lib;
          # Keep in sync with `rust-version` in Cargo.toml.
          rustToolchain = pkgs.rust-bin.stable."1.89.0".default.override {
            extensions = [
              "rust-src"
              "rust-analyzer"
              "clippy"
            ];
          };
          craneLib = (inputs.crane.mkLib pkgs).overrideToolchain rustToolchain;

          # Libraries that Blitz loads at runtime via `dlopen` (winit + wgpu).
          # They are not needed to *build*, but a Blitz app needs them on
          # `LD_LIBRARY_PATH` to open a window and talk to the GPU.
          runtimeLibs = lib.optionals pkgs.stdenv.isLinux [
            pkgs.wayland
            pkgs.libxkbcommon
            pkgs.libGL
            pkgs.vulkan-loader
            pkgs.libx11
            pkgs.libxcursor
            pkgs.libxi
            pkgs.libxrandr
            pkgs.libxcb
          ];

          # Dependencies needed to build Blitz:
          #   * openssl     -> openssl-sys, via reqwest in blitz-net
          #   * fontconfig  -> yeslogic-fontconfig-sys, via parley/fontique
          rustBuildInputs = [
            pkgs.openssl
          ]
          ++ lib.optionals pkgs.stdenv.isLinux (
            [ pkgs.fontconfig ] ++ runtimeLibs
          )
          ++ lib.optionals pkgs.stdenv.isDarwin [
            pkgs.apple-sdk
            pkgs.libiconv
          ];

          # `python3` is required at build time to generate code for `stylo`.
          rustNativeBuildInputs = [
            pkgs.pkg-config
            pkgs.python3
          ];

          cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
          fullSrc = pkgs.lib.cleanSource ./.;

          commonArgs = {
            src = fullSrc;
            strictDeps = true;
            buildInputs = rustBuildInputs;
            nativeBuildInputs = rustNativeBuildInputs;
          };

          rustPackage =
            package:
            {
              binary ? package,
              features ? [ ],
            }:
            craneLib.buildPackage (
              commonArgs
              // {
                pname = package;
                version = cargoToml.workspace.package.version;
                # Blitz's workspace root is a *virtual* manifest (no root
                # `[package]`), so build deps and the crate together in one
                # derivation instead of crane's dummy deps-only layer.
                cargoArtifacts = null;
                cargoExtraArgs = "--locked --package ${package} ${
                  lib.concatStringsSep " " (map (f: "--features ${f}") features)
                }";
                doCheck = false; # Disable tests to avoid building deps for them
                nativeBuildInputs = rustNativeBuildInputs ++ [ pkgs.makeWrapper ];
                installPhaseCommand = ''
                  mkdir -p $out/bin
                  cp target/release/${binary} $out/bin/
                ''
                + lib.optionalString pkgs.stdenv.isLinux ''
                  wrapProgram $out/bin/${binary} \
                    --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeLibs}
                '';
              }
            );
        in
        {
          # The example browser app (`apps/browser`), whose binary is `blitz`.
          packages.browser = rustPackage "browser" {
            binary = "blitz";
          };
          packages.default = self'.packages.browser;

          devShells.default = pkgs.mkShell {
            name = "blitz-dev";
            buildInputs = rustBuildInputs;
            nativeBuildInputs = rustNativeBuildInputs ++ [
              rustToolchain
            ];
            shellHook = ''
              # For rust-analyzer 'hover' tooltips to work.
              export RUST_SRC_PATH="${rustToolchain}/lib/rustlib/src/rust/library";
            ''
            + lib.optionalString pkgs.stdenv.isLinux ''
              # Blitz dlopen's these at runtime; make them discoverable when
              # running examples/apps from inside the dev shell.
              export LD_LIBRARY_PATH="${lib.makeLibraryPath runtimeLibs}:$LD_LIBRARY_PATH"
            '';
          };
        };
    };
}
