// The 3D model viewer page (port of ModelViewer.tsx, lib/modelLoader.ts, lib/modelStage.ts).
// Bundled with three.js into viewer.js by build.sh; the app serves it with the model bytes.

import {
  Box3,
  DirectionalLight,
  DoubleSide,
  GridHelper,
  HemisphereLight,
  Line,
  LoadingManager,
  MathUtils,
  Mesh,
  MeshStandardMaterial,
  PerspectiveCamera,
  Points,
  PointsMaterial,
  Scene,
  Sphere,
  Texture,
  Vector3,
  WebGLRenderer,
} from 'three'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'
import { OBJLoader } from 'three/addons/loaders/OBJLoader.js'
import { PLYLoader } from 'three/addons/loaders/PLYLoader.js'
import { STLLoader } from 'three/addons/loaders/STLLoader.js'
import { ThreeMFLoader } from 'three/addons/loaders/3MFLoader.js'

const Y_UP = 'y'
const Z_UP = 'z'
const SURFACE_COLOR = 0xc5cad3
const FIELD_OF_VIEW = 45

/** For formats that carry geometry but no material of their own. */
function surfaceMaterial(geometry) {
  const hasColors = geometry.hasAttribute('color')
  return new MeshStandardMaterial({
    color: hasColors ? 0xffffff : SURFACE_COLOR,
    vertexColors: hasColors,
    roughness: 0.6,
    metalness: 0.1,
    // Open meshes and inconsistent winding are common; show back faces rather than holes.
    side: DoubleSide,
  })
}

const isMesh = (object) => object instanceof Mesh
const isPoints = (object) => object instanceof Points
const isDrawable = (object) => isMesh(object) || isPoints(object) || object instanceof Line
const materialsOf = (object) => (Array.isArray(object.material) ? object.material : [object.material])

function parseStl(data) {
  const geometry = new STLLoader().parse(data)
  // Stored facet normals are often zero or stale; derive them from the winding instead.
  geometry.computeVertexNormals()
  return { object: new Mesh(geometry, surfaceMaterial(geometry)), upAxis: Z_UP, skippedFiles: [] }
}

function parseObj(data) {
  const object = new OBJLoader().parse(new TextDecoder().decode(data))
  // OBJLoader leaves its placeholder material on every mesh (it does not follow `mtllib`).
  object.traverse((child) => {
    if (!isMesh(child)) return
    for (const material of materialsOf(child)) material.dispose()
    child.material = surfaceMaterial(child.geometry)
  })
  return { object, upAxis: Y_UP, skippedFiles: [] }
}

function parsePly(data) {
  const geometry = new PLYLoader().parse(data)
  if (geometry.index === null) {
    // No faces: a scanned point cloud, its points sized to the cloud.
    const hasColors = geometry.hasAttribute('color')
    const material = new PointsMaterial({ vertexColors: hasColors, color: hasColors ? 0xffffff : SURFACE_COLOR })
    const cloud = new Points(geometry, material)
    material.size = new Box3().setFromObject(cloud).getSize(new Vector3()).length() / 500
    return { object: cloud, upAxis: Y_UP, skippedFiles: [] }
  }
  if (!geometry.hasAttribute('normal')) geometry.computeVertexNormals()
  return { object: new Mesh(geometry, surfaceMaterial(geometry)), upAxis: Y_UP, skippedFiles: [] }
}

function parse3mf(data) {
  return { object: new ThreeMFLoader().parse(data), upAxis: Z_UP, skippedFiles: [] }
}

function parseGltf(data) {
  // Only embedded data can load: the viewer reads one file. A refused buffer fails the
  // parse; a refused texture is dropped and reported.
  const skippedFiles = new Set()
  const manager = new LoadingManager()
  manager.setURLModifier((url) => {
    if (url.startsWith('data:') || url.startsWith('blob:')) return url
    skippedFiles.add(url)
    throw new Error(
      `This model references a separate file, "${url}", which the viewer cannot load. ` +
      'Only self-contained .glb files and .gltf files with embedded data can be shown.',
    )
  })
  return new Promise((resolve, reject) => {
    new GLTFLoader(manager).parse(data, '', (gltf) => {
      resolve({ object: gltf.scene, upAxis: Y_UP, skippedFiles: Array.from(skippedFiles) })
    }, reject)
  })
}

const PARSERS = { stl: parseStl, obj: parseObj, ply: parsePly, '3mf': parse3mf, glb: parseGltf, gltf: parseGltf }

async function parseModel(format, data) {
  const parse = PARSERS[format]
  if (!parse) throw new Error(`Unknown model format "${format}"`)
  const model = await parse(data)
  if (new Box3().setFromObject(model.object).isEmpty()) {
    disposeModel(model.object)
    throw new Error('The file contains no geometry to show.')
  }
  return model
}

function measureModel(object) {
  let triangles = 0
  let points = 0
  object.traverse((child) => {
    if (isMesh(child)) triangles += (child.geometry.index ?? child.geometry.getAttribute('position')).count / 3
    else if (isPoints(child)) points += child.geometry.getAttribute('position').count
  })
  return { triangles, points, size: new Box3().setFromObject(object).getSize(new Vector3()) }
}

function disposeModel(object) {
  object.traverse((child) => {
    if (!isDrawable(child)) return
    child.geometry.dispose()
    for (const material of materialsOf(child)) {
      for (const value of Object.values(material)) {
        if (value instanceof Texture) value.dispose()
      }
      material.dispose()
    }
  })
}

const UP_VECTORS = { [Y_UP]: new Vector3(0, 1, 0), [Z_UP]: new Vector3(0, 0, 1) }
// Front-right and above. Y-up models face +Z; Z-up (printing) models face -Y.
const VIEW_DIRECTIONS = {
  [Y_UP]: new Vector3(1, 0.8, 1).normalize(),
  [Z_UP]: new Vector3(1, -1, 0.8).normalize(),
}

/** Orbit (drag), pan (right-drag) and zoom (wheel); the camera takes the model's up axis. */
function createModelStage(container, model) {
  const renderer = new WebGLRenderer({ antialias: true, alpha: true })
  renderer.setPixelRatio(window.devicePixelRatio)
  container.appendChild(renderer.domElement)

  const up = UP_VECTORS[model.upAxis]
  const scene = new Scene()
  const sky = new HemisphereLight(0xffffff, 0x404040, 1.5)
  sky.position.copy(up)
  scene.add(sky)

  const camera = new PerspectiveCamera(FIELD_OF_VIEW, 1, 0.1, 1000)
  camera.up.copy(up) // OrbitControls fixes its orbit axis from camera.up when constructed.
  camera.add(new DirectionalLight(0xffffff, 1.5)) // a headlight
  scene.add(camera)
  scene.add(model.object)

  const bounds = new Box3().setFromObject(model.object)
  const center = bounds.getCenter(new Vector3())
  const radius = bounds.getBoundingSphere(new Sphere()).radius
  const extent = bounds.getSize(new Vector3())

  // A floor under the model, so its orientation reads at a glance.
  const grid = new GridHelper(Math.max(extent.x, extent.y, extent.z) * 2, 20, 0x5a5a5a, 0x3a3a3a)
  if (model.upAxis === Z_UP) grid.rotation.x = Math.PI / 2
  grid.position.copy(center).addScaledVector(up, bounds.min.dot(up) - center.dot(up))
  scene.add(grid)

  const controls = new OrbitControls(camera, renderer.domElement)
  const render = () => renderer.render(scene, camera)
  controls.addEventListener('change', render)

  const resetView = () => {
    const distance = (radius / Math.sin(MathUtils.degToRad(FIELD_OF_VIEW / 2))) * 1.1
    camera.near = distance / 100
    camera.far = distance * 100
    camera.updateProjectionMatrix()
    camera.position.copy(center).addScaledVector(VIEW_DIRECTIONS[model.upAxis], distance)
    controls.target.copy(center)
    controls.update()
    render()
  }

  const resize = () => {
    const { clientWidth, clientHeight } = container
    if (clientWidth === 0 || clientHeight === 0) return
    renderer.setSize(clientWidth, clientHeight, false)
    camera.aspect = clientWidth / clientHeight
    camera.updateProjectionMatrix()
    render()
  }
  new ResizeObserver(resize).observe(container)
  resetView()

  return {
    resetView,
    setWireframe: (enabled) => {
      model.object.traverse((child) => {
        if (!isMesh(child)) return
        for (const material of materialsOf(child)) {
          if ('wireframe' in material) material.wireframe = enabled
        }
      })
      render()
    },
  }
}

const formatCount = (count) => Math.round(count).toLocaleString()
const formatLength = (length) => String(Number(length.toPrecision(4)))
const errorMessage = (err) => (err instanceof Error ? err.message : String(err))

/** Tells the app how loading went (it logs it): `ready …` or `error …`. */
function report(message) {
  if (window.ipc) window.ipc.postMessage(message)
}

function showError(text) {
  document.getElementById('status').textContent = text
  document.getElementById('status').className = 'error'
  report(`error ${text}`)
}

async function main() {
  const format = new URLSearchParams(window.location.search).get('format') ?? ''
  let model
  try {
    const response = await fetch('model')
    if (!response.ok) throw new Error(`HTTP ${response.status}`)
    model = await parseModel(format, await response.arrayBuffer())
  } catch (err) {
    showError(`Could not read model: ${errorMessage(err)}`)
    return
  }
  document.getElementById('status').remove()
  let stage
  try {
    stage = createModelStage(document.getElementById('canvas'), model)
  } catch (err) {
    showError(`3D view unavailable: ${errorMessage(err)}`)
    return
  }
  const { triangles, points, size } = measureModel(model.object)
  const counts = [
    triangles > 0 ? `${formatCount(triangles)} triangles` : '',
    points > 0 ? `${formatCount(points)} points` : '',
  ].filter(Boolean).join(' · ')
  document.getElementById('stats').textContent =
    `${counts} · ${formatLength(size.x)} × ${formatLength(size.y)} × ${formatLength(size.z)}`
  report(`ready ${document.getElementById('stats').textContent}`)
  if (model.skippedFiles.length > 0) {
    const warning = document.getElementById('warning')
    warning.textContent = `Shown without the separate files it references: ${model.skippedFiles.join(', ')}`
    warning.hidden = false
  }
  let wireframe = false
  const toolbar = document.getElementById('toolbar')
  toolbar.hidden = false
  document.getElementById('reset').addEventListener('click', () => stage.resetView())
  document.getElementById('wireframe').addEventListener('click', (event) => {
    wireframe = !wireframe
    stage.setWireframe(wireframe)
    event.currentTarget.classList.toggle('active', wireframe)
  })
}

main()
