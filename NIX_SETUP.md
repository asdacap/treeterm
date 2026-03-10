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
- Node.js 20.x (LTS)
- npm
- Python 3 (for node-gyp)
- All native build dependencies for Electron
- Proper library paths configured automatically

### Build the Application

Build the application using Nix:

```bash
nix build
```

The built application will be available in `./result/bin/treeterm`.

### Run the Application

Run directly without building:

```bash
nix run
```

Or run from a specific directory:

```bash
nix run . -- /path/to/directory
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

3. Allow direnv in this directory:
   ```bash
   direnv allow
   ```

Now the development environment will load automatically when you `cd` into the project!

## Common Commands

Inside the Nix development shell:

```bash
# Install dependencies
npm install

# Start development server
npm run dev

# Build the application
npm run build

# Run tests
npm test

# Run tests once (CI mode)
npm run test:run
```

## Platform Support

The flake supports:
- **Linux**: Full support with all Electron dependencies
- **macOS**: Full support with macOS-specific frameworks
- **Other Unix systems**: Basic support (may require additional configuration)

## Troubleshooting

### Native Module Build Failures

If you encounter issues building native modules (like `node-pty`):

1. Make sure you're inside the Nix development shell: `nix develop`
2. Clear npm cache: `npm cache clean --force`
3. Remove node_modules: `rm -rf node_modules`
4. Reinstall: `npm install`

### Electron Binary Issues on Linux

If Electron fails to start on Linux, ensure library paths are set:

```bash
export LD_LIBRARY_PATH=$(nix build --no-link --print-out-paths)/lib:$LD_LIBRARY_PATH
```

This is automatically configured in the dev shell.

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
  run: nix develop --command npm test
```

## Customization

The flake configuration is in `flake.nix`. Key customization points:

- **Node.js version**: Change `nodejs_20` to another version
- **Electron version**: Update `electron_33` to match package.json
- **Additional dependencies**: Add to `buildInputs` or `nativeBuildInputs`
- **Environment variables**: Modify in `shellHook`

## Learn More

- [Nix Flakes](https://nixos.wiki/wiki/Flakes)
- [Nix Pills](https://nixos.org/guides/nix-pills/)
- [direnv](https://direnv.net/)
