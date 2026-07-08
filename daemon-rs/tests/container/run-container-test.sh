#!/usr/bin/env bash
# Driver for the daemon container boot test.
#
# Compiles the daemon as a static musl binary and boots it inside stock
# ubuntu and fedora containers, verifying it actually starts on each distro.
# The verification runs as a `RUN` step during the image build (see Dockerfile),
# so a successful build == a passing test.
#
# Usage:
#   run-container-test.sh              # test ubuntu and fedora
#   run-container-test.sh ubuntu       # test a single distro
#   run-container-test.sh ubuntu fedora
#
# Requires: docker (with BuildKit).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
DOCKERFILE="$SCRIPT_DIR/Dockerfile"

if ! command -v docker >/dev/null 2>&1; then
  echo "ERROR: docker is required to run the container boot test" >&2
  exit 1
fi

distros=("$@")
if [ "${#distros[@]}" -eq 0 ]; then
  distros=(ubuntu fedora)
fi

export DOCKER_BUILDKIT=1

fail=0
for distro in "${distros[@]}"; do
  echo "############################################################"
  echo "# Building + booting treeterm-daemon on: ${distro}"
  echo "############################################################"
  if docker build \
      --progress=plain \
      --file "$DOCKERFILE" \
      --target "${distro}-test" \
      --tag "treeterm-daemon-boot-${distro}" \
      "$REPO_ROOT"; then
    echo ">>> ${distro}: PASS"
  else
    echo ">>> ${distro}: FAIL"
    fail=1
  fi
done

echo "############################################################"
if [ "$fail" -eq 0 ]; then
  echo "# All distro boot tests PASSED"
else
  echo "# One or more distro boot tests FAILED"
fi
echo "############################################################"
exit "$fail"
