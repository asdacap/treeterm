// pdf.js's viewer components read the library from `globalThis.pdfjsLib`: set before they load.
import * as pdfjsLib from 'pdfjs-dist/legacy/build/pdf.mjs'

globalThis.pdfjsLib = pdfjsLib
