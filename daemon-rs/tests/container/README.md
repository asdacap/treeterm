# Daemon container boot test

Verifies that the built `treeterm-daemon` binary can actually **start on a stock
Linux distro** (Ubuntu and Fedora), not just on the developer's machine. This
guards the remote-daemon path: the daemon is shipped as a static musl binary and
run over SSH on arbitrary homelab boxes, so it must boot without any distro
support libraries.

## What it checks

For each distro the [`verify-daemon-boot.sh`](./verify-daemon-boot.sh) smoke test
boots the daemon and asserts it:

1. is executable (and, best-effort, statically linked),
2. binds its unix socket,
3. logs readiness (`daemon listening`),
4. writes its pid file under `$HOME/.treeterm`,
5. stays alive after startup,
6. shuts down cleanly on `SIGINT`.

## How it works

[`Dockerfile`](./Dockerfile) compiles the daemon as a static musl binary in an
`rust:alpine` builder stage, then copies that single binary onto `ubuntu:24.04`
and `fedora:latest` and runs the smoke test as a build `RUN` step. A successful
image build means the boot test passed.

## Running

```sh
npm run test:daemon:container            # ubuntu + fedora
daemon-rs/tests/container/run-container-test.sh ubuntu   # single distro
```

Requires Docker with BuildKit. The Rust compile is cached across runs via
BuildKit cache mounts, so repeat runs only re-run the boot check. In CI it runs
as the `daemon-container-boot` job.
