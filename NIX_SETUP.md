# Nix Flake Setup for TreeTerm

This project includes Nix flake support for reproducible development environments and builds.

## Prerequisites

1. Install Nix with flakes enabled:
   ```bash
   sh <(curl -L https://nixos.org/nix/install) --daemon
   ```

2. Enable flakes by adding to `~/.config/nix/nix.conf`:
   ```
   experimental-features = nix-command flakes
   ```

## Quick Start

### Development Environment

Enter a development shell with all dependencies:

```bash
nix develop
```

This provides:
- A stable Rust toolchain with the `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` targets, for the static daemons uploaded to remote Linux hosts
- musl cross linkers for both targets
- `protoc` and `pkg-config`
- On Linux, the libraries the app links (fontconfig, freetype, xkbcommon, Wayland, X11, Vulkan, GTK 3, WebKitGTK 4.1), with `LD_LIBRARY_PATH` set for those it loads at run time

The shell has no rustup, so the toolchain pinned in `app-rs/rust-toolchain.toml` is not applied there.

On macOS, build the app outside the shell, with rustup and Xcode: GPUI compiles its Metal shaders with Xcode's `metal`.

### Build the Application (Linux)

```bash
nix build
```

This builds the app with the toolchain pinned in `app-rs/rust-toolchain.toml`, and the local daemon next to it. The result is `./result/bin/treeterm`, plus a `.desktop` entry and icon under `./result/share/`. It does not include the static daemons for remote hosts; `cargo run -p xtask -- dist` (in `app-rs/`) builds a tarball that does.

`.#treeterm-native` and `.#treeterm` are the same package.

### Run the Application (Linux)

Run directly without installing:

```bash
nix run
```

Or open a directory as a workspace:

```bash
nix run . -- /path/to/directory
```

### Build the Daemon

```bash
nix build .#daemon            # this platform, linked dynamically
(cd daemon-rs && nix build)   # Linux only: a static musl daemon (daemon-rs/flake.nix)
```

## direnv Integration (Optional)

For automatic environment loading when entering the project directory:

1. Install direnv:
   ```bash
   # On macOS
   brew install direnv

   # On NixOS/Linux with Nix
   nix profile install nixpkgs#direnv
   ```

2. Add to your shell rc file (`~/.bashrc`, `~/.zshrc`, etc.):
   ```bash
   eval "$(direnv hook bash)"  # or zsh, fish, etc.
   ```

3. Create an `.envrc` containing `use flake`, then allow it:
   ```bash
   echo "use flake" > .envrc
   direnv allow
   ```

Now the development environment will load automatically when you `cd` into the project!

## Common Commands

Inside the Nix development shell:

```bash
# Build the daemon
(cd daemon-rs && cargo build)

# Build and run the app
(cd app-rs && cargo build -p treeterm-app && ./target/debug/treeterm)

# Run tests
(cd daemon-rs && cargo test)
(cd app-rs && cargo test --workspace)

# Build the daemons (local and remote) and package the app
(cd app-rs && cargo run -p xtask -- dist)
```

## Platform Support

- **Linux**: dev shell, the app package (`nix build`, `nix run`) and the daemon packages
- **macOS**: dev shell (for the daemon) and `.#daemon`; build the app with rustup and Xcode

## CI/CD Integration

Use the flake in GitHub Actions or other CI:

```yaml
- name: Install Nix
  uses: cachix/install-nix-action@v22
  with:
    extra_nix_config: |
      experimental-features = nix-command flakes

- name: Build
  run: nix build

- name: Test
  run: nix develop --command sh -c 'cd daemon-rs && cargo test'
```

## Customization

The flake configuration is in `flake.nix`. Key customization points:

- **Dev shell toolchain**: `rustToolchain` (targets and version)
- **App toolchain**: `app-rs/rust-toolchain.toml`, which `treeterm-native` builds with
- **App libraries**: `nativeAppLibs` (linked) and `nativeRuntimeLibs` (loaded at run time)
- **Additional dependencies**: Add to `devNativeBuildInputs`

## Learn More

- [Nix Flakes](https://nixos.wiki/wiki/Flakes)
- [Nix Pills](https://nixos.org/guides/nix-pills/)
- [direnv](https://direnv.net/)
