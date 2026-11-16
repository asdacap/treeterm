// The PDF viewer page: Chromium's PDF viewer (which Electron's PdfViewer shows a PDF in) on
// pdf.js — its page box, zoom, fit, rotate, download and print, and the sidebar of thumbnails
// and the document outline. Bundled with pdf.js into viewer.js by build.sh; the app serves it
// with the PDF's bytes (`document.pdf`) and its file name (`name`), does what the page asks
// of it (`print`, `download`) and shows a print that failed here (`print-failed <reason>`).

import './pdfjs-global.js'
import 'pdfjs-dist/legacy/web/pdf_viewer.css'
import { EventBus, LinkTarget, PDFLinkService, PDFViewer } from 'pdfjs-dist/legacy/web/pdf_viewer.mjs'

const pdfjsLib = globalThis.pdfjsLib
pdfjsLib.GlobalWorkerOptions.workerSrc = 'pdf.worker.mjs'

/** Chromium's zoom presets (`kPresetZoomFactors`): the zoom buttons step through them. */
const ZOOM_FACTORS = [0.25, 1 / 3, 0.5, 2 / 3, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5]
const MIN_ZOOM = ZOOM_FACTORS[0]
const MAX_ZOOM = ZOOM_FACTORS[ZOOM_FACTORS.length - 1]
/** Within a hair of a preset counts as on it (1/3 and 2/3 are not exact). */
const EPSILON = 1e-6
const THUMBNAIL_WIDTH = 140
/** Pages are printed as images at this many dots per inch (pdf.js's print service). */
const PRINT_DPI = 150

/** The zoom one step from `current`: the next preset that way, staying at the ends. */
export function zoomStep(current, direction) {
  const next =
    direction > 0
      ? ZOOM_FACTORS.find((factor) => factor > current + EPSILON)
      : [...ZOOM_FACTORS].reverse().find((factor) => factor < current - EPSILON)
  return next ?? (direction > 0 ? MAX_ZOOM : MIN_ZOOM)
}

/** The page a typed page number names, or null when it names none of `count`. */
export function parsePage(text, count) {
  const page = Number(text.trim())
  return Number.isInteger(page) && page >= 1 && page <= count ? page : null
}

/** The zoom a typed "125%" or "125" names, kept within Chromium's range, or null. */
export function parseZoom(text) {
  const percent = Number(text.trim().replace(/%$/, ''))
  if (!Number.isFinite(percent) || percent <= 0) return null
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, percent / 100))
}

function report(message) {
  if (window.ipc) window.ipc.postMessage(message)
}

const element = (id) => document.getElementById(id)

function show(id, text) {
  const node = element(id)
  if (text !== undefined) node.textContent = text
  node.hidden = false
}

const errorMessage = (err) => (err instanceof Error ? err.message : String(err))

async function main() {
  const name = await fetch('name').then((response) => response.text())
  element('title').textContent = name
  document.title = name
  // Download saves the file as it is on disk, through the app's save panel.
  element('download').addEventListener('click', () => report('download'))

  let pdf
  try {
    const bytes = new Uint8Array(await (await fetch('document.pdf')).arrayBuffer())
    pdf = await pdfjsLib.getDocument({ data: bytes, isEvalSupported: false }).promise
  } catch (err) {
    element('loading').hidden = true
    show('load-error', `Could not load the PDF: ${errorMessage(err)}`)
    report(`error ${errorMessage(err)}`)
    return
  }
  const title = (await pdf.getMetadata().catch(() => null))?.info?.Title
  if (typeof title === 'string' && title.trim() !== '') {
    element('title').textContent = title
    document.title = title
  }

  const eventBus = new EventBus()
  // Links out of the document open in the system browser (a new window is refused there).
  const linkService = new PDFLinkService({ eventBus, externalLinkTarget: LinkTarget.BLANK })
  const viewer = new PDFViewer({ container: element('viewerContainer'), eventBus, linkService })
  linkService.setViewer(viewer)
  const thumbnails = createThumbnails(pdf, viewer)
  viewer.setDocument(pdf)
  linkService.setDocument(pdf, null)

  const pageInput = element('page-input')
  const zoomInput = element('zoom-input')
  // Chromium opens a PDF fitted to the width, and the button then offers the whole page.
  let fitting = 'page-width'
  const showFitting = () => {
    const toPage = fitting !== 'page-fit'
    element('fit').title = toPage ? 'Fit to page' : 'Fit to width'
    // SVG elements have no `hidden` property: the attribute hides them.
    element('fit-page-icon').toggleAttribute('hidden', !toPage)
    element('fit-width-icon').toggleAttribute('hidden', toPage)
  }
  const showZoom = () => {
    zoomInput.value = `${Math.round(viewer.currentScale * 100)}%`
    element('zoom-out').disabled = viewer.currentScale <= MIN_ZOOM + EPSILON
    element('zoom-in').disabled = viewer.currentScale >= MAX_ZOOM - EPSILON
  }
  const zoomTo = (scale) => {
    fitting = null
    viewer.currentScale = scale
    showFitting()
  }

  eventBus.on('pagesinit', () => {
    viewer.currentScaleValue = fitting
    element('loading').hidden = true
    show('center')
    element('page-count').textContent = `/ ${pdf.numPages}`
    pageInput.value = String(viewer.currentPageNumber)
    element('sidebar-toggle').disabled = false
    element('print').disabled = false
    showFitting()
    showZoom()
    report(`ready ${pdf.numPages} pages`)
  })
  eventBus.on('pagechanging', ({ pageNumber }) => {
    if (document.activeElement !== pageInput) pageInput.value = String(pageNumber)
    thumbnails.setCurrent(pageNumber)
  })
  eventBus.on('scalechanging', showZoom)
  eventBus.on('rotationchanging', () => thumbnails.reset())
  // The view keeps its fitting as the window resizes, as Chromium's does.
  new ResizeObserver(() => {
    if (fitting !== null) viewer.currentScaleValue = fitting
  }).observe(element('viewerContainer'))

  pageInput.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter') return
    const page = parsePage(pageInput.value, pdf.numPages)
    if (page !== null) viewer.currentPageNumber = page
    pageInput.value = String(viewer.currentPageNumber)
    pageInput.blur()
  })
  pageInput.addEventListener('focus', () => pageInput.select())
  pageInput.addEventListener('blur', () => {
    pageInput.value = String(viewer.currentPageNumber)
  })
  zoomInput.addEventListener('keydown', (event) => {
    if (event.key !== 'Enter') return
    const zoom = parseZoom(zoomInput.value)
    if (zoom !== null) zoomTo(zoom)
    showZoom()
    zoomInput.blur()
  })
  zoomInput.addEventListener('focus', () => zoomInput.select())
  zoomInput.addEventListener('blur', showZoom)
  element('zoom-out').addEventListener('click', () => zoomTo(zoomStep(viewer.currentScale, -1)))
  element('zoom-in').addEventListener('click', () => zoomTo(zoomStep(viewer.currentScale, 1)))
  element('fit').addEventListener('click', () => {
    fitting = fitting === 'page-fit' ? 'page-width' : 'page-fit'
    viewer.currentScaleValue = fitting
    showFitting()
  })
  element('rotate').addEventListener('click', () => {
    viewer.pagesRotation = (viewer.pagesRotation + 270) % 360
    if (fitting !== null) viewer.currentScaleValue = fitting
  })
  element('print').addEventListener('click', async () => {
    element('print').disabled = true
    try {
      await preparePrint(pdf, viewer.pagesRotation)
      report('print')
    } catch (err) {
      // The app shows it above the page, as it shows a print panel that cannot open.
      report(`print-failed ${errorMessage(err)}`)
    } finally {
      element('print').disabled = false
    }
  })

  const outline = await pdf.getOutline().catch(() => null)
  if (outline && outline.length > 0) {
    element('outline').append(outlineList(outline, linkService))
    show('sidebar-tabs')
  }
  const selectTab = (tab) => {
    element('thumbnails').hidden = tab !== 'thumbnails'
    element('outline').hidden = tab !== 'outline'
    element('show-thumbnails').classList.toggle('selected', tab === 'thumbnails')
    element('show-outline').classList.toggle('selected', tab === 'outline')
  }
  element('show-thumbnails').addEventListener('click', () => selectTab('thumbnails'))
  element('show-outline').addEventListener('click', () => selectTab('outline'))
  element('sidebar-toggle').addEventListener('click', (event) => {
    const open = element('sidebar').hidden
    element('sidebar').hidden = !open
    event.currentTarget.setAttribute('aria-pressed', String(open))
    if (fitting !== null) viewer.currentScaleValue = fitting
    if (open) thumbnails.setCurrent(viewer.currentPageNumber)
  })
}

/** The sidebar's page thumbnails, each drawn once it scrolls into view. */
function createThumbnails(pdf, viewer) {
  const list = element('thumbnails')
  const canvases = []
  const drawn = new Set()
  const observer = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (entry.isIntersecting) draw(Number(entry.target.dataset.page))
    }
  }, { root: list })
  async function draw(number) {
    if (drawn.has(number)) return
    drawn.add(number)
    const page = await pdf.getPage(number)
    const rotation = (page.rotate + viewer.pagesRotation) % 360
    const unscaled = page.getViewport({ scale: 1, rotation })
    const ratio = window.devicePixelRatio || 1
    const viewport = page.getViewport({ scale: (THUMBNAIL_WIDTH / unscaled.width) * ratio, rotation })
    const canvas = canvases[number - 1]
    canvas.width = Math.floor(viewport.width)
    canvas.height = Math.floor(viewport.height)
    await page.render({ canvasContext: canvas.getContext('2d'), viewport }).promise
  }
  for (let number = 1; number <= pdf.numPages; number++) {
    const button = document.createElement('button')
    button.className = 'thumbnail'
    button.title = `Page ${number}`
    button.dataset.page = String(number)
    const canvas = document.createElement('canvas')
    // A placeholder of a portrait page until it is drawn.
    canvas.width = THUMBNAIL_WIDTH
    canvas.height = Math.round(THUMBNAIL_WIDTH * 1.294)
    canvases.push(canvas)
    const label = document.createElement('span')
    label.textContent = String(number)
    button.append(canvas, label)
    button.addEventListener('click', () => {
      viewer.currentPageNumber = number
    })
    list.append(button)
    observer.observe(button)
  }
  return {
    setCurrent(number) {
      for (const button of list.children) {
        const current = Number(button.dataset.page) === number
        button.classList.toggle('current', current)
        if (current && !element('sidebar').hidden) button.scrollIntoView({ block: 'nearest' })
      }
    },
    /** Pages rotated: every thumbnail is drawn again as it shows. */
    reset() {
      drawn.clear()
      for (const button of list.children) {
        observer.unobserve(button)
        observer.observe(button)
      }
    },
  }
}

function outlineList(items, linkService) {
  const list = document.createElement('div')
  for (const item of items) {
    const button = document.createElement('button')
    button.className = 'outline-item'
    button.textContent = item.title
    button.title = item.title
    button.addEventListener('click', () => {
      if (item.dest) linkService.goToDestination(item.dest)
      else if (item.url) window.open(item.url)
    })
    list.append(button)
    if (item.items && item.items.length > 0) {
      const children = outlineList(item.items, linkService)
      children.className = 'outline-children'
      list.append(children)
    }
  }
  return list
}

/** Every page drawn as an image for the print layout (`@media print`), as pdf.js prints. */
async function preparePrint(pdf, pagesRotation) {
  const container = element('printContainer')
  container.textContent = ''
  for (let number = 1; number <= pdf.numPages; number++) {
    const page = await pdf.getPage(number)
    const viewport = page.getViewport({ scale: PRINT_DPI / 72, rotation: (page.rotate + pagesRotation) % 360 })
    const canvas = document.createElement('canvas')
    canvas.width = Math.floor(viewport.width)
    canvas.height = Math.floor(viewport.height)
    const context = canvas.getContext('2d')
    context.fillStyle = 'white'
    context.fillRect(0, 0, canvas.width, canvas.height)
    await page.render({ canvasContext: context, viewport, intent: 'print' }).promise
    const image = document.createElement('img')
    image.src = canvas.toDataURL('image/png')
    await image.decode()
    container.append(image)
  }
}

main().catch((err) => {
  element('loading').hidden = true
  show('load-error', `Could not show the PDF: ${errorMessage(err)}`)
  report(`error ${errorMessage(err)}`)
})
