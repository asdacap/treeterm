{
  description = "TreeTerm - Hierarchical terminal manager and IDE for AI agent workflows";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };

        # Rust toolchain with musl cross-compilation targets (the dev shell's)
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          targets = [
            "x86_64-unknown-linux-musl"
            "aarch64-unknown-linux-musl"
          ];
        };

        # Cross-compilation linkers for musl targets
        muslCrossX86 = pkgs.pkgsCross.musl64.stdenv.cc;
        muslCrossAarch64 = pkgs.pkgsCross.aarch64-multiplatform-musl.stdenv.cc;

        # Dev shell build inputs
        devNativeBuildInputs = with pkgs; [
          pkg-config
          rustToolchain
          protobuf
          muslCrossX86
          muslCrossAarch64
        ];

        # Build the Rust daemon separately
        treeterm-daemon = pkgs.rustPlatform.buildRustPackage {
          pname = "treeterm-daemon";
          version = "0.1.0";

          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              let
                relPath = pkgs.lib.removePrefix (toString ./.) (toString path);
              in
              # daemon-rs/ only (its proto is in crates/treeterm-proto/proto/)
              pkgs.lib.hasPrefix "/daemon-rs" relPath
              || relPath == "";
          };

          sourceRoot = "source/daemon-rs";

          cargoLock.lockFile = ./daemon-rs/Cargo.lock;

          nativeBuildInputs = [ pkgs.protobuf ];

          doCheck = false;

          # Disable the musl cross-compilation config for nix build
          preBuild = ''
            rm -f .cargo/config.toml
          '';
        };

        # Native Rust/GPUI app (app-rs/), Linux only: macOS bundles come from
        # `cargo run -p xtask -- bundle` (GPUI compiles its Metal shaders with Xcode's `metal`).
        # GPUI needs a newer Rust than nixpkgs ships, so it builds with the toolchain app-rs
        # pins, from rust-overlay.
        appRustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./app-rs/rust-toolchain.toml;
        appRustPlatform = pkgs.makeRustPlatform {
          cargo = appRustToolchain;
          rustc = appRustToolchain;
        };

        # Linked by the native app on Linux (also in the dev shell, to build app-rs there).
        nativeAppLibs = with pkgs; [
          fontconfig
          freetype
          libxkbcommon
          wayland
          vulkan-loader
          libx11
          libxcb
          libxrandr
          # Webview panes (wry): WebKitGTK 4.1 on GTK 3; glib-networking gives it TLS.
          gtk3
          webkitgtk_4_1
          libsoup_3
          glib-networking
        ];

        # Loaded with dlopen at run time (GPU, display servers, keymaps): added to the RUNPATH
        # (and to LD_LIBRARY_PATH in the dev shell).
        nativeRuntimeLibs = with pkgs; [
          vulkan-loader
          libGL
          wayland
          libxkbcommon
          libx11
          libxcb
          libxcursor
          libxi
          libxrandr
        ];

        treeterm-native = appRustPlatform.buildRustPackage {
          pname = "treeterm-native";
          version = "0.1.0";

          src = pkgs.lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              let
                relPath = pkgs.lib.removePrefix (toString ./.) (toString path);
                under = dir: relPath == dir || pkgs.lib.hasPrefix "${dir}/" relPath;
              in
              # app-rs/ with its path dependencies in daemon-rs/ (the proto among them)
              relPath == ""
              || (under "/app-rs" && !under "/app-rs/target")
              || (under "/daemon-rs" && !under "/daemon-rs/target");
          };

          sourceRoot = "source/app-rs";

          # Registry crates only (no git sources), so the lockfile alone vendors them.
          cargoLock.lockFile = ./app-rs/Cargo.lock;
          cargoBuildFlags = [ "-p" "treeterm-app" ];

          # The tests need a daemon, sockets and a GPU; `cargo test` in app-rs/ runs them.
          doCheck = false;

          nativeBuildInputs = with pkgs; [
            pkg-config
            protobuf
            wrapGAppsHook3
          ];

          buildInputs = nativeAppLibs;

          # Only the app gets the GTK environment, not the daemon next to it.
          dontWrapGApps = true;

          # Same layout as the Linux tarball: the daemon next to the executable, where
          # treeterm-app's daemon_binary.rs looks (it resolves the bin/ symlink).
          postInstall = ''
            mkdir -p $out/libexec/treeterm
            mv $out/bin/treeterm $out/libexec/treeterm/treeterm
            cp ${treeterm-daemon}/bin/treeterm-daemon $out/libexec/treeterm/
            ln -s ../libexec/treeterm/treeterm $out/bin/treeterm
            install -Dm644 packaging/linux/treeterm.desktop $out/share/applications/treeterm.desktop
            install -Dm644 packaging/treeterm.svg $out/share/icons/hicolor/scalable/apps/treeterm.svg
          '';

          # After the fixup phase's --shrink-rpath, which would drop the dlopen-only entries.
          postFixup = ''
            patchelf --add-rpath ${pkgs.lib.makeLibraryPath nativeRuntimeLibs} $out/libexec/treeterm/treeterm
            wrapGApp $out/libexec/treeterm/treeterm
          '';

          meta = with pkgs.lib; {
            description = "TreeTerm native app (Rust/GPUI)";
            license = licenses.mit;
            platforms = platforms.linux;
            mainProgram = "treeterm";
          };
        };

      in
      pkgs.lib.recursiveUpdate
      {
        # Development shell
        devShells.default = pkgs.mkShell {
          buildInputs = pkgs.lib.optionals pkgs.stdenv.isLinux nativeAppLibs;
          nativeBuildInputs = devNativeBuildInputs;

          shellHook = ''
            echo "TreeTerm development environment"
            echo "Rust: $(rustc --version)"
            echo ""
            echo "Available commands:"
            echo "  (cd daemon-rs && cargo build)                 - Build the daemon"
            echo "  (cd app-rs && cargo build -p treeterm-app)    - Build the app"
            echo "  (cd daemon-rs && cargo test)                  - Daemon tests"
            echo "  (cd app-rs && cargo test --workspace)         - App tests"
            echo "  (cd app-rs && cargo run -p xtask -- dist)     - Build the daemons and package the app"
            echo ""
          '' + pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            # Libraries the app opens with dlopen (GPU, display servers, keymaps)
            export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath nativeRuntimeLibs}:$LD_LIBRARY_PATH
          '';
        };

        # `nix build .#daemon`
        packages.daemon = treeterm-daemon;
      }
      (pkgs.lib.optionalAttrs pkgs.stdenv.isLinux {
        # `nix build`, `nix run` (also as .#treeterm-native)
        packages.default = treeterm-native;
        packages.treeterm = treeterm-native;
        packages.treeterm-native = treeterm-native;
        apps.default = {
          type = "app";
          program = "${treeterm-native}/bin/treeterm";
        };
        apps.treeterm-native = self.apps.${system}.default;
      })
    );
}
