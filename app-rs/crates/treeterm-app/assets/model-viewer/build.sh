#!/bin/sh
# Rebuild viewer.js (the 3D model viewer page's script) from viewer.src.js and three.js.
# Needs `three` and `esbuild` resolvable from here: the repo's node_modules while it exists,
# otherwise `npm install --no-save three@0.186.1 esbuild` in this directory first.
set -eu
cd "$(dirname "$0")"
ESBUILD="${ESBUILD:-$(command -v esbuild || echo ../../../../../node_modules/.bin/esbuild)}"
"$ESBUILD" viewer.src.js --bundle --minify --format=iife --target=safari15 \
  --legal-comments=eof --outfile=viewer.js
