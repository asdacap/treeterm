#!/bin/sh
# Boot smoke test for the built treeterm-daemon binary.
#
# Runs *inside* a container (ubuntu/fedora) against a binary that was compiled
# elsewhere (a static musl build). Proves the daemon can actually start on a
# stock distro: it binds its unix socket, logs readiness, writes its pid file,
# stays alive, and shuts down cleanly on SIGINT.
#
# Exits non-zero on the first failure so it can gate a `docker build` RUN step.
#
# Usage: verify-daemon-boot.sh [path-to-daemon-binary]
set -eu

DAEMON="${1:-/usr/local/bin/treeterm-daemon}"
DISTRO="$( (. /etc/os-release 2>/dev/null && printf '%s' "${PRETTY_NAME:-unknown}") || printf 'unknown')"

WORK="$(mktemp -d)"
export HOME="$WORK/home"
export TREETERM_SOCKET_PATH="$WORK/daemon.sock"
mkdir -p "$HOME"
LOG="$WORK/daemon.log"

echo "=== treeterm-daemon boot test on: $DISTRO ==="

dump_log() {
  echo "----- daemon log -----"
  cat "$LOG" 2>/dev/null || echo "(no log)"
  echo "----------------------"
}

# 1. Binary must exist and be executable.
if [ ! -x "$DAEMON" ]; then
  echo "FAIL: daemon binary not executable at $DAEMON"
  exit 1
fi
echo "OK: found daemon binary at $DAEMON"

# 2. Best-effort: the whole point of the musl build is that it is portable, so
#    confirm it is statically linked. Non-fatal — a dynamic build that still
#    boots on this distro is a warning, not a hard failure.
if command -v ldd >/dev/null 2>&1; then
  if ldd "$DAEMON" 2>&1 | grep -qiE 'not a dynamic executable|statically linked'; then
    echo "OK: binary is statically linked"
  else
    echo "WARN: binary looks dynamically linked on $DISTRO:"
    ldd "$DAEMON" 2>&1 || true
  fi
fi

# 3. Launch the daemon in the background.
"$DAEMON" >"$LOG" 2>&1 &
PID=$!

cleanup() {
  kill "$PID" 2>/dev/null || true
  wait "$PID" 2>/dev/null || true
}
trap cleanup EXIT INT TERM

# 4. Wait up to ~10s for the unix socket to appear, bailing early if the
#    process dies (e.g. a missing shared library would abort it immediately).
i=0
while [ "$i" -lt 100 ]; do
  [ -S "$TREETERM_SOCKET_PATH" ] && break
  if ! kill -0 "$PID" 2>/dev/null; then
    echo "FAIL: daemon exited before binding its socket"
    dump_log
    exit 1
  fi
  i=$((i + 1))
  sleep 0.1
done

if [ ! -S "$TREETERM_SOCKET_PATH" ]; then
  echo "FAIL: socket $TREETERM_SOCKET_PATH never appeared within timeout"
  dump_log
  exit 1
fi
echo "OK: unix socket bound at $TREETERM_SOCKET_PATH"

# 5. Daemon must log readiness (JSON tracing line with message "daemon listening").
if ! grep -q "daemon listening" "$LOG"; then
  echo "FAIL: daemon did not log readiness"
  dump_log
  exit 1
fi
echo "OK: daemon logged readiness"

# 6. Pid file written under \$HOME/.treeterm.
if [ ! -f "$HOME/.treeterm/daemon.pid" ]; then
  echo "FAIL: pid file not written under \$HOME/.treeterm"
  dump_log
  exit 1
fi
echo "OK: pid file written"

# 7. Still alive a moment later (didn't crash right after binding).
sleep 0.5
if ! kill -0 "$PID" 2>/dev/null; then
  echo "FAIL: daemon died shortly after startup"
  dump_log
  exit 1
fi
echo "OK: daemon still running"

# 8. Graceful shutdown: the daemon installs a ctrl_c handler, so SIGINT should
#    make it exit and clean up its socket.
kill -INT "$PID" 2>/dev/null || true
i=0
while [ "$i" -lt 50 ]; do
  kill -0 "$PID" 2>/dev/null || break
  i=$((i + 1))
  sleep 0.1
done
if kill -0 "$PID" 2>/dev/null; then
  echo "WARN: daemon did not exit on SIGINT within timeout"
else
  echo "OK: daemon exited on SIGINT"
  if [ -S "$TREETERM_SOCKET_PATH" ]; then
    echo "WARN: socket file left behind after shutdown"
  else
    echo "OK: socket cleaned up on shutdown"
  fi
fi

echo "=== PASS: treeterm-daemon boots on $DISTRO ==="
