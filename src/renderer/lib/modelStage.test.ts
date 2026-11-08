// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { BoxGeometry, GridHelper, Mesh, MeshStandardMaterial, PerspectiveCamera, Vector3 } from 'three'
import { UpAxis, type LoadedModel } from './modelLoader'
import { createModelStage } from './modelStage'

// jsdom has no WebGL: a renderer that records what it is asked to do.
const renderers = vi.hoisted(() => [] as Array<{
  domElement: HTMLCanvasElement
  setPixelRatio: ReturnType<typeof vi.fn>
  setSize: ReturnType<typeof vi.fn>
  render: ReturnType<typeof vi.fn>
  dispose: ReturnType<typeof vi.fn>
  forceContextLoss: ReturnType<typeof vi.fn>
}>)

vi.mock('three', async (importOriginal) => {
  const actual = await importOriginal<typeof import('three')>()
  class FakeWebGLRenderer {
    domElement = document.createElement('canvas')
    setPixelRatio = vi.fn()
    setSize = vi.fn()
    render = vi.fn()
    dispose = vi.fn()
    forceContextLoss = vi.fn()
    constructor() {
      renderers.push(this)
    }
  }
  return { ...actual, WebGLRenderer: FakeWebGLRenderer }
})

class FakeResizeObserver {
  static instances: FakeResizeObserver[] = []
  observe = vi.fn()
  disconnect = vi.fn()
  constructor(private readonly callback: () => void) {
    FakeResizeObserver.instances.push(this)
  }
  fire(): void {
    this.callback()
  }
}

function boxModel(upAxis: UpAxis): LoadedModel {
  // A 2 × 4 × 6 box whose minimum corner sits at the origin.
  const geometry = new BoxGeometry(2, 4, 6)
  geometry.translate(1, 2, 3)
  return { object: new Mesh(geometry, new MeshStandardMaterial()), upAxis, skippedFiles: [] }
}

function sizedContainer(width: number, height: number): HTMLDivElement {
  const container = document.createElement('div')
  Object.defineProperty(container, 'clientWidth', { configurable: true, value: width })
  Object.defineProperty(container, 'clientHeight', { configurable: true, value: height })
  document.body.appendChild(container)
  return container
}

function lastCamera(): PerspectiveCamera {
  const call = renderers[0]!.render.mock.lastCall
  expect(call).toBeDefined()
  return call![1] as PerspectiveCamera
}

describe('createModelStage', () => {
  beforeEach(() => {
    renderers.length = 0
    FakeResizeObserver.instances = []
    vi.stubGlobal('ResizeObserver', FakeResizeObserver)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
    document.body.innerHTML = ''
  })

  it('frames a Y-up model from the front-right and above, looking at its centre', () => {
    const container = sizedContainer(800, 600)
    const model = boxModel(UpAxis.Y)

    createModelStage(container, model)

    expect(container.querySelector('canvas')).toBe(renderers[0]!.domElement)
    const camera = lastCamera()
    expect(camera.up.toArray()).toEqual([0, 1, 0])
    const offset = camera.position.clone().sub(new Vector3(1, 2, 3)).normalize()
    expect(offset.x).toBeCloseTo(offset.z)
    expect(offset.y).toBeGreaterThan(0)
    expect(model.object.parent).not.toBeNull()
  })

  it('orbits a Z-up model about Z and lays the floor grid under it in the XY plane', () => {
    const model = boxModel(UpAxis.Z)

    createModelStage(sizedContainer(800, 600), model)

    const camera = lastCamera()
    expect(camera.up.toArray()).toEqual([0, 0, 1])
    // Looks from the front (-Y), above (+Z).
    expect(camera.position.y).toBeLessThan(2)
    expect(camera.position.z).toBeGreaterThan(3)
    const scene = model.object.parent!
    const grid = scene.children.find((child) => child instanceof GridHelper)!
    expect(grid.rotation.x).toBeCloseTo(Math.PI / 2)
    expect(grid.position.toArray()).toEqual([1, 2, 0])
  })

  it('sizes the canvas to the container, and keeps its size while the container is hidden', () => {
    const container = sizedContainer(800, 400)
    createModelStage(container, boxModel(UpAxis.Y))
    const renderer = renderers[0]!
    const observer = FakeResizeObserver.instances[0]!
    expect(observer.observe).toHaveBeenCalledWith(container)

    observer.fire()
    expect(renderer.setSize).toHaveBeenCalledWith(800, 400, false)
    expect(lastCamera().aspect).toBe(2)

    Object.defineProperty(container, 'clientWidth', { configurable: true, value: 0 })
    observer.fire()
    expect(renderer.setSize).toHaveBeenCalledTimes(1)
  })

  it('toggles wireframe on the model materials and redraws', () => {
    const model = boxModel(UpAxis.Y)
    const stage = createModelStage(sizedContainer(800, 600), model)
    const renders = renderers[0]!.render.mock.calls.length

    stage.setWireframe(true)

    expect((model.object as Mesh).material).toMatchObject({ wireframe: true })
    expect(renderers[0]!.render.mock.calls.length).toBe(renders + 1)
    stage.setWireframe(false)
    expect((model.object as Mesh).material).toMatchObject({ wireframe: false })
  })

  it('returns the camera to the framed view on reset', () => {
    const stage = createModelStage(sizedContainer(800, 600), boxModel(UpAxis.Y))
    const framed = lastCamera().position.clone()

    lastCamera().position.set(50, 50, 50)
    stage.resetView()

    expect(lastCamera().position.distanceTo(framed)).toBeCloseTo(0)
  })

  it('releases the canvas, the WebGL context and the model on dispose', () => {
    const container = sizedContainer(800, 600)
    const model = boxModel(UpAxis.Y)
    const stage = createModelStage(container, model)

    stage.dispose()

    const renderer = renderers[0]!
    expect(container.querySelector('canvas')).toBeNull()
    expect(renderer.dispose).toHaveBeenCalledOnce()
    expect(renderer.forceContextLoss).toHaveBeenCalledOnce()
    expect(FakeResizeObserver.instances[0]!.disconnect).toHaveBeenCalledOnce()
    expect(model.object.parent).toBeNull()
  })

  it('refuses a container outside any window', () => {
    const detached = document.implementation.createHTMLDocument('').createElement('div')

    expect(() => createModelStage(detached, boxModel(UpAxis.Y))).toThrow('The viewer is not attached to a window')
  })
})
