import {
  Box3,
  DirectionalLight,
  GridHelper,
  HemisphereLight,
  MathUtils,
  PerspectiveCamera,
  Scene,
  Sphere,
  Vector3,
  WebGLRenderer,
} from 'three'
import { OrbitControls } from 'three/addons/controls/OrbitControls.js'
import { UpAxis, isMesh, materialsOf, type LoadedModel } from './modelLoader'

export interface ModelStage {
  /** Frames the whole model from the default three-quarter angle. */
  resetView: () => void
  setWireframe: (enabled: boolean) => void
  dispose: () => void
}

const UP_VECTORS: Record<UpAxis, Vector3> = {
  [UpAxis.Y]: new Vector3(0, 1, 0),
  [UpAxis.Z]: new Vector3(0, 0, 1),
}

// Front-right and above. Y-up models face +Z; Z-up (printing) models face -Y.
const VIEW_DIRECTIONS: Record<UpAxis, Vector3> = {
  [UpAxis.Y]: new Vector3(1, 0.8, 1).normalize(),
  [UpAxis.Z]: new Vector3(1, -1, 0.8).normalize(),
}

const FIELD_OF_VIEW = 45

/**
 * Draws `model` into a canvas appended to `container`, with orbit (drag), pan
 * (right-drag) and zoom (wheel). The model stays in its own coordinates: the camera
 * takes the model's up axis instead. Throws when WebGL is unavailable.
 */
export function createModelStage(container: HTMLElement, model: LoadedModel): ModelStage {
  const view = container.ownerDocument.defaultView
  if (!view) throw new Error('The viewer is not attached to a window')
  const renderer = new WebGLRenderer({ antialias: true, alpha: true })
  renderer.setPixelRatio(view.devicePixelRatio)
  container.appendChild(renderer.domElement)

  const up = UP_VECTORS[model.upAxis]
  const scene = new Scene()
  const sky = new HemisphereLight(0xffffff, 0x404040, 1.5)
  sky.position.copy(up)
  scene.add(sky)

  const camera = new PerspectiveCamera(FIELD_OF_VIEW, 1, 0.1, 1000)
  // OrbitControls fixes its orbit axis from camera.up when constructed.
  camera.up.copy(up)
  // A headlight: whatever face the viewer looks at is lit.
  camera.add(new DirectionalLight(0xffffff, 1.5))
  scene.add(camera)
  scene.add(model.object)

  const bounds = new Box3().setFromObject(model.object)
  const center = bounds.getCenter(new Vector3())
  const radius = bounds.getBoundingSphere(new Sphere()).radius
  const extent = bounds.getSize(new Vector3())

  // A floor under the model, so its orientation reads at a glance.
  const grid = new GridHelper(Math.max(extent.x, extent.y, extent.z) * 2, 20, 0x5a5a5a, 0x3a3a3a)
  if (model.upAxis === UpAxis.Z) grid.rotation.x = Math.PI / 2
  grid.position.copy(center).addScaledVector(up, bounds.min.dot(up) - center.dot(up))
  scene.add(grid)

  const controls = new OrbitControls(camera, renderer.domElement)
  const render = (): void => { renderer.render(scene, camera) }
  controls.addEventListener('change', render)

  const resetView = (): void => {
    const distance = (radius / Math.sin(MathUtils.degToRad(FIELD_OF_VIEW / 2))) * 1.1
    camera.near = distance / 100
    camera.far = distance * 100
    camera.updateProjectionMatrix()
    camera.position.copy(center).addScaledVector(VIEW_DIRECTIONS[model.upAxis], distance)
    controls.target.copy(center)
    controls.update()
    render()
  }

  const resize = (): void => {
    const { clientWidth, clientHeight } = container
    // An inactive tab is display:none; keep the last size until it shows again.
    if (clientWidth === 0 || clientHeight === 0) return
    renderer.setSize(clientWidth, clientHeight, false)
    camera.aspect = clientWidth / clientHeight
    camera.updateProjectionMatrix()
    render()
  }
  const resizeObserver = new ResizeObserver(resize)
  resizeObserver.observe(container)
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
    dispose: () => {
      resizeObserver.disconnect()
      controls.dispose()
      scene.remove(model.object)
      grid.dispose()
      renderer.dispose()
      // Chromium caps live WebGL contexts; release this one now rather than at GC.
      renderer.forceContextLoss()
      renderer.domElement.remove()
    },
  }
}
