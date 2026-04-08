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
          config.allowUnfree = true; # Electron is unfree
          overlays = [ rust-overlay.overlays.default ];
        };

        # Node.js LTS (22.x) - must use pkgs.nodejs, not nodejs_20,
        # because nodejs_20's npm has a broken nodejs-slim prefix that
        # fails when loaded via direnv.
        nodejs = pkgs.nodejs;

        # Rust toolchain with musl cross-compilation targets
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
          nodejs
          (python3.withPackages (ps: [ ps.setuptools ]))
          pkg-config
          makeWrapper
          rustToolchain
          protobuf
          muslCrossX86
          muslCrossAarch64
        ];

        electronLibs = with pkgs; [
          stdenv.cc.cc.lib
        ] ++ lib.optionals stdenv.isLinux [
          alsa-lib
          at-spi2-atk
          at-spi2-core
          cairo
          cups
          dbus
          expat
          gdk-pixbuf
          glib
          gtk3
          libdrm
          libnotify
          libpulseaudio
          libuuid
          libxkbcommon
          mesa
          nspr
          nss
          pango
          systemd
          libx11
          libxscrnsaver
          libxcomposite
          libxcursor
          libxdamage
          libxext
          libxfixes
          libxi
          libxrandr
          libxrender
          libxtst
          libxcb
          libxshmfence
        ] ++ lib.optionals stdenv.isDarwin [
          darwin.apple_sdk.frameworks.CoreServices
          darwin.apple_sdk.frameworks.AppKit
          darwin.apple_sdk.frameworks.Security
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
              # Include daemon-rs/ and src/proto/ (needed by build.rs)
              pkgs.lib.hasPrefix "/daemon-rs" relPath
              || pkgs.lib.hasPrefix "/src/proto" relPath
              || relPath == "/daemon-rs"
              || relPath == "/src"
              || relPath == "/src/proto"
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

      in
      {
        # Development shell
        devShells.default = pkgs.mkShell {
          buildInputs = electronLibs;
          nativeBuildInputs = devNativeBuildInputs;

          shellHook = ''
            echo "TreeTerm development environment"
            echo "Node.js: $(node --version)"
            echo "npm: $(npm --version)"
            echo ""
            echo "Available commands:"
            echo "  npm install      - Install dependencies"
            echo "  npm run dev      - Start development server"
            echo "  npm run build    - Build the application"
            echo "  npm test         - Run tests"
            echo ""
          '' + pkgs.lib.optionalString pkgs.stdenv.isLinux ''
            # Set up library path for Electron on Linux
            export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath electronLibs}:$LD_LIBRARY_PATH
          '';

          # Environment variables
          ELECTRON_SKIP_BINARY_DOWNLOAD = "1";
          ELECTRON_OVERRIDE_DIST_PATH = "${pkgs.electron}/bin/";
          ELECTRON_EXEC_PATH = "${pkgs.electron}/bin/electron";
          NODE_OPTIONS = "--max-old-space-size=4096";
        };

        # Package definition
        packages.default = pkgs.buildNpmPackage {
          pname = "treeterm";
          version = "0.1.0";

          src = ./.;

          npmDepsHash = "sha256-u3yIfD37HB//ael83BJ8/lilJ/IFTYfJqhPhsQ15CIc=";
          npmFlags = [ "--legacy-peer-deps" "--ignore-scripts" ];

          nativeBuildInputs = with pkgs; [
            nodejs
            (python3.withPackages (ps: [ ps.setuptools ]))
            pkg-config
            makeWrapper
          ];

          buildInputs = electronLibs;

          makeCacheWritable = true;

          # Proto files are pre-generated in src/generated/
          # Daemon is built separately as treeterm-daemon
          # Only run electron-vite build here
          buildPhase = ''
            runHook preBuild

            # Compile proto client for CLI usage (bin/treeterm.js needs it)
            mkdir -p out/daemon/generated
            npx tsc src/generated/treeterm.ts \
              --outDir out/daemon/generated \
              --module commonjs \
              --target es2020 \
              --esModuleInterop \
              --declaration \
              --skipLibCheck \
              --moduleResolution node

            npx electron-vite build
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall

            mkdir -p $out/lib/treeterm
            cp -r out $out/lib/treeterm/out
            cp -r node_modules $out/lib/treeterm/
            cp package.json $out/lib/treeterm/

            # Install daemon binary
            mkdir -p $out/lib/treeterm/out/daemon-rs
            cp ${treeterm-daemon}/bin/treeterm-daemon $out/lib/treeterm/out/daemon-rs/

            # Install the CLI wrapper
            mkdir -p $out/bin $out/lib/treeterm/bin
            cp bin/treeterm.js $out/lib/treeterm/bin/

            makeWrapper ${nodejs}/bin/node $out/bin/treeterm \
              --add-flags "$out/lib/treeterm/bin/treeterm.js" \
              --set ELECTRON_OVERRIDE_DIST_PATH "${pkgs.electron}/bin/" \
              ${pkgs.lib.optionalString pkgs.stdenv.isLinux
                "--prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath electronLibs}"}

            runHook postInstall
          '';

          meta = with pkgs.lib; {
            description = "Hierarchical terminal manager and IDE for AI agent workflows";
            license = licenses.mit;
            platforms = platforms.unix;
            mainProgram = "treeterm";
          };
        };

        # Expose daemon as a separate package
        packages.daemon = treeterm-daemon;

        # Alias for the package
        packages.treeterm = self.packages.${system}.default;

        # App definition (for `nix run`)
        apps.default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/treeterm";
        };
      }
    );
}
