{
  description = "Tool for managing root CAs stored on offline storage";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };

    crane.url = "github:ipetkov/crane";
  };

  outputs = { self, nixpkgs, rust-overlay, crane, ... }:
    let systems = [ "x86_64-linux"
                    "aarch64-linux" ];
        forAllSystems = f: nixpkgs.lib.genAttrs systems (system:
          let pkgs = import nixpkgs {
                inherit system;
                overlays = [ rust-overlay.overlays.default ];
              };
              rust = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          in f system pkgs rust);
    in {
      packages = forAllSystems (system: pkgs: rust:
        let craneLib = (crane.mkLib pkgs).overrideToolchain rust;

            src = craneLib.cleanCargoSource ./.;

            commonArgs = {
              inherit src;
              strictDeps = true;
              nativeBuildInputs = [ pkgs.pkg-config ];
              buildInputs = [ pkgs.openssl pkgs.zbar ];
            };

            cargoArtifacts = craneLib.buildDepsOnly commonArgs;

            package = craneLib.buildPackage (commonArgs // {
              inherit cargoArtifacts;
            });

        in {
          default = package;
          pkiboo = package;
        });

      overlays.default = final: prev: {
        inherit (self.packages.${final.stdenv.hostPlatform.system}) pkiboo;
      };

      devShells = forAllSystems (system: pkgs: rust: {
        default = pkgs.mkShell {
          packages = [
            rust
            pkgs.rust-analyzer
            pkgs.pkg-config
            pkgs.claude-code

            pkgs.codex
            pkgs.codex-acp

          ];
          buildInputs = [
            pkgs.openssl.dev
            pkgs.udev.dev
            pkgs.zbar.dev
	  ];
          RUST_SRC_PATH = "${rust}/lib/rustlib/src/rust/library";
        };
      });
    };
}
