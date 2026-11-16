#!/bin/sh
# Rebuild the PDF viewer page's script (viewer.js, viewer.css) from viewer.src.js and pdf.js,
# and copy pdf.js's worker (pdf.worker.mjs). pdf.js's legacy build: WKWebView is the system's
# WebKit, which may be older than the modern build needs.
# Needs `pdfjs-dist` and `esbuild` resolvable from here: `npm install --no-save
# pdfjs-dist@4.10.38 esbuild` in this directory first (or NODE_PATH naming a node_modules
# that has them).
set -eu
cd "$(dirname "$0")"
ESBUILD="${ESBUILD:-$(command -v esbuild || echo ../../../../../node_modules/.bin/esbuild)}"
"$ESBUILD" viewer.src.js --bundle --minify --format=esm --target=safari15 \
  --loader:.svg=dataurl --loader:.gif=dataurl --legal-comments=eof \
  --log-override:empty-import-meta=silent --outfile=viewer.js
PDFJS="$(node -p "require('path').dirname(require.resolve('pdfjs-dist/package.json'))")"
cp "$PDFJS/legacy/build/pdf.worker.min.mjs" pdf.worker.mjs
