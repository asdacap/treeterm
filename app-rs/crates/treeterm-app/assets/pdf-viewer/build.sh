#!/bin/sh
# Rebuild the PDF viewer page's script (viewer.js, viewer.css) from viewer.src.js and pdf.js,
# and copy pdf.js's worker (pdf.worker.mjs), its CMaps (cmaps/, for text in a predefined CJK
# CMap the PDF does not embed) and its standard fonts (standard_fonts/, for the standard fonts
# a PDF does not embed). pdf.js's legacy build: WKWebView is the system's WebKit, which may be
# older than the modern build needs. The app's build.rs lists cmaps/ and standard_fonts/ for
# the page to serve.
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
rm -rf cmaps standard_fonts
mkdir cmaps standard_fonts
cp "$PDFJS"/cmaps/*.bcmap "$PDFJS/cmaps/LICENSE" cmaps/
cp "$PDFJS"/standard_fonts/*.pfb "$PDFJS"/standard_fonts/*.ttf "$PDFJS"/standard_fonts/LICENSE_* standard_fonts/
