// @vitest-environment jsdom
import { act, fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { BoxGeometry, BufferGeometry, Float32BufferAttribute, Mesh, MeshStandardMaterial, Points } from 'three'
import { ModelFormat } from '../../shared/fileContentKind'
import { UpAxis, type LoadedModel } from '../lib/modelLoader'
import ModelViewer from './ModelViewer'

const loader = vi.hoisted(() => ({ parseModel: vi.fn() }))
vi.mock('../lib/modelLoader', async (importOriginal) => ({
  ...await importOriginal<typeof import('../lib/modelLoader')>(),
  parseModel: loader.parseModel,
}))

// jsdom has no WebGL; the stage is stood in for.
const stage = vi.hoisted(() => ({
  createModelStage: vi.fn(),
  instance: { resetView: vi.fn(), setWireframe: vi.fn(), dispose: vi.fn() },
}))
vi.mock('../lib/modelStage', () => ({ createModelStage: stage.createModelStage }))

function boxModel(skippedFiles: string[] = []): LoadedModel {
  return { object: new Mesh(new BoxGeometry(10, 20, 30), new MeshStandardMaterial()), upAxis: UpAxis.Z, skippedFiles }
}

const STL_BASE64 = btoa('solid x')

describe('ModelViewer', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    stage.createModelStage.mockReturnValue(stage.instance)
  })

  it('parses the decoded bytes, then shows the model with its size and triangle count', async () => {
    loader.parseModel.mockResolvedValue(boxModel())

    render(<ModelViewer base64={STL_BASE64} format={ModelFormat.Stl} />)

    expect(screen.getByText('Reading model…')).toBeTruthy()
    expect(await screen.findByText('12 triangles · 10 × 20 × 30')).toBeTruthy()
    const [format, data] = loader.parseModel.mock.calls[0] as [ModelFormat, ArrayBuffer]
    expect(format).toBe(ModelFormat.Stl)
    expect(new TextDecoder().decode(data)).toBe('solid x')
    expect(stage.createModelStage).toHaveBeenCalledOnce()
  })

  it('counts the points of a point cloud', async () => {
    const geometry = new BufferGeometry()
    geometry.setAttribute('position', new Float32BufferAttribute([0, 0, 0, 1, 2, 3], 3))
    loader.parseModel.mockResolvedValue({ object: new Points(geometry), upAxis: UpAxis.Y, skippedFiles: [] })

    render(<ModelViewer base64={STL_BASE64} format={ModelFormat.Ply} />)

    expect(await screen.findByText('2 points · 1 × 2 × 3')).toBeTruthy()
  })

  it('shows why the model could not be read', async () => {
    loader.parseModel.mockRejectedValue(new Error('bad header'))

    render(<ModelViewer base64={STL_BASE64} format={ModelFormat.Stl} />)

    expect(await screen.findByText('Could not read model: bad header')).toBeTruthy()
    expect(stage.createModelStage).not.toHaveBeenCalled()
  })

  it('shows a decoding failure of the transferred bytes as a read error', async () => {
    render(<ModelViewer base64="%%%" format={ModelFormat.Stl} />)

    expect(await screen.findByText(/^Could not read model: /)).toBeTruthy()
    expect(loader.parseModel).not.toHaveBeenCalled()
  })

  it('shows that the 3D view is unavailable when the stage cannot start', async () => {
    loader.parseModel.mockResolvedValue(boxModel())
    stage.createModelStage.mockImplementation(() => { throw new Error('WebGL is off') })

    render(<ModelViewer base64={STL_BASE64} format={ModelFormat.Stl} />)

    expect(await screen.findByText('3D view unavailable: WebGL is off')).toBeTruthy()
    expect(screen.queryByText('Reset view')).toBeNull()
  })

  it('resets the view and toggles wireframe through the stage', async () => {
    loader.parseModel.mockResolvedValue(boxModel())
    render(<ModelViewer base64={STL_BASE64} format={ModelFormat.Stl} />)

    fireEvent.click(await screen.findByText('Reset view'))
    expect(stage.instance.resetView).toHaveBeenCalledOnce()

    const wireframe = screen.getByText('Wireframe').closest('button')!
    fireEvent.click(wireframe)
    expect(stage.instance.setWireframe).toHaveBeenLastCalledWith(true)
    expect(wireframe.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(wireframe)
    expect(stage.instance.setWireframe).toHaveBeenLastCalledWith(false)
    expect(wireframe.getAttribute('aria-pressed')).toBe('false')
  })

  it('lists the referenced files it showed the model without', async () => {
    loader.parseModel.mockResolvedValue(boxModel(['wood.png', 'normal.png']))

    render(<ModelViewer base64={STL_BASE64} format={ModelFormat.Gltf} />)

    expect(await screen.findByText('Shown without the separate files it references: wood.png, normal.png')).toBeTruthy()
  })

  it('disposes the stage and the model on unmount', async () => {
    const model = boxModel()
    const disposeGeometry = vi.spyOn((model.object as Mesh).geometry, 'dispose')
    loader.parseModel.mockResolvedValue(model)
    const { unmount } = render(<ModelViewer base64={STL_BASE64} format={ModelFormat.Stl} />)
    await screen.findByText('Reset view')

    unmount()

    expect(stage.instance.dispose).toHaveBeenCalledOnce()
    expect(disposeGeometry).toHaveBeenCalledOnce()
  })

  it('disposes a model that finishes parsing after the viewer is gone', async () => {
    const model = boxModel()
    const disposeGeometry = vi.spyOn((model.object as Mesh).geometry, 'dispose')
    let finish: (model: LoadedModel) => void = () => {}
    loader.parseModel.mockReturnValue(new Promise<LoadedModel>((resolve) => { finish = resolve }))
    const { unmount } = render(<ModelViewer base64={STL_BASE64} format={ModelFormat.Stl} />)

    unmount()
    finish(model)
    await act(async () => { await Promise.resolve() })

    expect(disposeGeometry).toHaveBeenCalledOnce()
    expect(stage.createModelStage).not.toHaveBeenCalled()
  })

  it('drops a parse failure that lands after the viewer is gone', async () => {
    let fail: (error: Error) => void = () => {}
    loader.parseModel.mockReturnValue(new Promise<LoadedModel>((_resolve, reject) => { fail = reject }))
    const { unmount } = render(<ModelViewer base64={STL_BASE64} format={ModelFormat.Stl} />)

    unmount()
    fail(new Error('late'))
    await act(async () => { await Promise.resolve() })

    expect(screen.queryByText(/late/)).toBeNull()
  })
})
