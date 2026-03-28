{
  description = "TreeTerm Daemon - Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, rust-overlay }:
    flake-utils.lib.eachSystem [ "x86_64-linux" "aarch64-linux" ] (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        # Rust toolchain with musl target
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          targets = [ "x86_64-unknown-linux-musl" "aarch64-unknown-linux-musl" ];
        };

        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        # Proto file path for build.rs
        protoSrc = ../src/proto;

        rustTarget = {
          "x86_64-linux" = "x86_64-unknown-linux-musl";
          "aarch64-linux" = "aarch64-unknown-linux-musl";
        }.${system};

        commonArgs = {
          src = craneLib.cleanCargoSource ./.;
          strictDeps = true;
          nativeBuildInputs = [ pkgs.protobuf ];
          # build.rs needs to find the proto file
          preBuild = ''
            mkdir -p src/proto
            cp -r ${protoSrc}/* src/proto/ || true
          '';
          CARGO_BUILD_TARGET = rustTarget;
          CARGO_BUILD_RUSTFLAGS = "-C target-feature=+crt-static";
        };

        daemon = craneLib.buildPackage (commonArgs // {
          cargoArtifacts = craneLib.buildDepsOnly commonArgs;
        });
      in
      {
        packages.default = daemon;
        packages.treeterm-daemon = daemon;

        devShells.default = craneLib.devShell {
          packages = [ pkgs.protobuf ];
        };
      }
    );
}
