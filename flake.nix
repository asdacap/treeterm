{
  description = "TreeTerm - Hierarchical terminal manager and IDE for AI agent workflows";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, nixpkgs, flake-utils }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          config.allowUnfree = true; # Electron is unfree
        };

        # Node.js LTS (22.x) - must use pkgs.nodejs, not nodejs_20,
        # because nodejs_20's npm has a broken nodejs-slim prefix that
        # fails when loaded via direnv.
        nodejs = pkgs.nodejs;

        # Build inputs needed for native dependencies and Rust daemon
        nativeBuildInputs = with pkgs; [
          nodejs
          (python3.withPackages (ps: [ ps.setuptools ])) # Required by node-gyp for native modules
          pkg-config
          makeWrapper

          # Rust toolchain for daemon-rs
          rustc
          cargo
          protobuf # protoc for tonic-build
        ];

        buildInputs = with pkgs; [
          # Libraries needed for Electron and native modules
          stdenv.cc.cc.lib
        ] ++ lib.optionals stdenv.isLinux [
          # Linux-specific dependencies for Electron
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
          # macOS-specific dependencies
          darwin.apple_sdk.frameworks.CoreServices
          darwin.apple_sdk.frameworks.AppKit
          darwin.apple_sdk.frameworks.Security
        ];

      in
      {
        # Development shell
        devShells.default = pkgs.mkShell {
          inherit buildInputs nativeBuildInputs;

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
            export LD_LIBRARY_PATH=${pkgs.lib.makeLibraryPath buildInputs}:$LD_LIBRARY_PATH
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

          npmDepsHash = "sha256-/6esp6OPZkomSm1eltNKtk23PYYJehOdHUll3XRUfhE=";
          npmFlags = [ "--legacy-peer-deps" ];

          inherit nativeBuildInputs buildInputs;

          # node-pty needs to compile native code
          makeCacheWritable = true;

          buildPhase = ''
            runHook preBuild
            npm run build
            runHook postBuild
          '';

          installPhase = ''
            runHook preInstall

            mkdir -p $out/lib/treeterm
            cp -r out/* $out/lib/treeterm/
            cp -r node_modules $out/lib/treeterm/
            cp package.json $out/lib/treeterm/

            # Install the CLI wrapper
            mkdir -p $out/bin
            cp bin/treeterm.js $out/lib/treeterm/bin/

            makeWrapper ${nodejs}/bin/node $out/bin/treeterm \
              --add-flags "$out/lib/treeterm/bin/treeterm.js" \
              ${pkgs.lib.optionalString pkgs.stdenv.isLinux
                "--prefix LD_LIBRARY_PATH : ${pkgs.lib.makeLibraryPath buildInputs}"}

            runHook postInstall
          '';

          meta = with pkgs.lib; {
            description = "Hierarchical terminal manager and IDE for AI agent workflows";
            homepage = "https://github.com/anthropics/treeterm";
            license = licenses.mit;
            platforms = platforms.unix;
            mainProgram = "treeterm";
          };
        };

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
