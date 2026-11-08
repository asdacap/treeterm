import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { JSDOM } from 'jsdom'
import { Mesh, MeshStandardMaterial, Points, PointsMaterial, Texture, BufferGeometry, Group, Float32BufferAttribute } from 'three'
import { strToU8, zipSync } from 'three/addons/libs/fflate.module.js'
import { ModelFormat } from '../../shared/fileContentKind'
import { UpAxis, disposeModel, isMesh, measureModel, parseModel } from './modelLoader'

function textData(text: string): ArrayBuffer {
  return new TextEncoder().encode(text).buffer
}

const ASCII_STL = `solid tri
facet normal 0 0 1
 outer loop
  vertex 0 0 0
  vertex 10 0 0
  vertex 0 20 0
 endloop
endfacet
endsolid tri
`

/**
 * A binary STL of `count` triangles with zeroed facet normals, as many exporters write them;
 * `colored` uses the Materialise per-face colour extension.
 */
function binaryStl(count: number, colored: boolean): ArrayBuffer {
  const buffer = new ArrayBuffer(84 + count * 50)
  const view = new DataView(buffer)
  if (colored) {
    const header = 'COLOR='
    for (let i = 0; i < header.length; i++) view.setUint8(i, header.charCodeAt(i))
    view.setUint32(6, 0xff8040ff)
  }
  view.setUint32(80, count, true)
  for (let face = 0; face < count; face++) {
    const offset = 84 + face * 50
    const vertices = [0, 0, face, 1, 0, face, 0, 1, face]
    vertices.forEach((v, i) => { view.setFloat32(offset + 12 + i * 4, v, true) })
    // Bit 15 clear: the face carries its own colour (pure red, 5 bits per channel).
    view.setUint16(offset + 48, colored ? 0x001f : 0, true)
  }
  return buffer
}

const OBJ = `v 0 0 0
v 1 0 0
v 0 1 0
v 0 0 1
f 1 2 3
f 1 2 4
`

const PLY_MESH = `ply
format ascii 1.0
element vertex 3
property float x
property float y
property float z
element face 1
property list uchar int vertex_indices
end_header
0 0 0
1 0 0
0 1 0
3 0 1 2
`

const PLY_POINTS = `ply
format ascii 1.0
element vertex 2
property float x
property float y
property float z
property uchar red
property uchar green
property uchar blue
end_header
0 0 0 255 0 0
4 3 0 0 255 0
`

// One triangle; its positions as glTF's little-endian float32 VEC3 accessor.
const TRIANGLE = new Float32Array([0, 0, 0, 1, 0, 0, 0, 2, 0])
const TRIANGLE_BYTES = new Uint8Array(TRIANGLE.buffer)

function gltfJson(buffer: { uri?: string }, extra: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    asset: { version: '2.0' },
    buffers: [{ byteLength: TRIANGLE_BYTES.byteLength, ...buffer }],
    bufferViews: [{ buffer: 0, byteLength: TRIANGLE_BYTES.byteLength }],
    accessors: [{ bufferView: 0, componentType: 5126, count: 3, type: 'VEC3', min: [0, 0, 0], max: [1, 2, 0] }],
    meshes: [{ primitives: [{ attributes: { POSITION: 0 } }] }],
    nodes: [{ mesh: 0 }],
    scenes: [{ nodes: [0] }],
    scene: 0,
    ...extra,
  }
}

function embeddedGltf(): ArrayBuffer {
  const uri = `data:application/octet-stream;base64,${btoa(String.fromCharCode(...Array.from(TRIANGLE_BYTES)))}`
  return textData(JSON.stringify(gltfJson({ uri })))
}

function glb(): ArrayBuffer {
  const json = new TextEncoder().encode(JSON.stringify(gltfJson({})))
  const jsonLength = Math.ceil(json.byteLength / 4) * 4
  const binLength = Math.ceil(TRIANGLE_BYTES.byteLength / 4) * 4
  const total = 12 + 8 + jsonLength + 8 + binLength
  const bytes = new Uint8Array(total)
  const view = new DataView(bytes.buffer)
  view.setUint32(0, 0x46546c67, true) // 'glTF'
  view.setUint32(4, 2, true)
  view.setUint32(8, total, true)
  view.setUint32(12, jsonLength, true)
  view.setUint32(16, 0x4e4f534a, true) // 'JSON'
  bytes.fill(0x20, 20, 20 + jsonLength)
  bytes.set(json, 20)
  view.setUint32(20 + jsonLength, binLength, true)
  view.setUint32(24 + jsonLength, 0x004e4942, true) // 'BIN'
  bytes.set(TRIANGLE_BYTES, 28 + jsonLength)
  return bytes.buffer
}

function threeMf(): ArrayBuffer {
  const rels = `<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>`
  const model = `<?xml version="1.0" encoding="UTF-8"?>
<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">
<resources><object id="1" type="model"><mesh>
<vertices><vertex x="0" y="0" z="0"/><vertex x="10" y="0" z="0"/><vertex x="0" y="20" z="0"/><vertex x="0" y="0" z="30"/></vertices>
<triangles><triangle v1="0" v2="1" v3="2"/><triangle v1="0" v2="1" v3="3"/></triangles>
</mesh></object></resources>
<build><item objectid="1"/></build>
</model>`
  const zipped = zipSync({ '_rels/.rels': strToU8(rels, true), '3D/3dmodel.model': strToU8(model, true) })
  return zipped.buffer.slice(zipped.byteOffset, zipped.byteOffset + zipped.byteLength)
}

/** Node lacks the ProgressEvent three's FileLoader reports fetch progress with. */
class NodeProgressEvent extends Event {
  constructor(type: string, init: { lengthComputable: boolean; loaded: number; total: number }) {
    super(type)
    Object.assign(this, init)
  }
}

function onlyMesh(object: unknown): Mesh {
  expect(object).toBeInstanceOf(Mesh)
  return object as Mesh
}

describe('parseModel', () => {
  // The loaders run in the renderer; give node the browser globals they reach for.
  beforeEach(() => {
    vi.stubGlobal('self', globalThis)
    vi.stubGlobal('ProgressEvent', NodeProgressEvent)
    vi.stubGlobal('DOMParser', new JSDOM('').window.DOMParser)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('reads an ASCII STL as a Z-up mesh with the neutral surface material', async () => {
    const model = await parseModel(ModelFormat.Stl, textData(ASCII_STL))

    expect(model.upAxis).toBe(UpAxis.Z)
    const mesh = onlyMesh(model.object)
    expect(mesh.material).toBeInstanceOf(MeshStandardMaterial)
    expect((mesh.material as MeshStandardMaterial).vertexColors).toBe(false)
    const { triangles, size } = measureModel(model.object)
    expect(triangles).toBe(1)
    expect(size.toArray()).toEqual([10, 20, 0])
  })

  it('reads a binary STL and shows its per-face colours', async () => {
    const model = await parseModel(ModelFormat.Stl, binaryStl(2, true))

    const mesh = onlyMesh(model.object)
    expect(measureModel(mesh).triangles).toBe(2)
    // The zeroed stored normals are replaced by ones derived from the counter-clockwise winding.
    expect(Array.from(mesh.geometry.getAttribute('normal').array.slice(0, 3))).toEqual([0, 0, 1])
    expect((mesh.material as MeshStandardMaterial).vertexColors).toBe(true)
    expect(mesh.geometry.hasAttribute('color')).toBe(true)
  })

  it('reads an OBJ as Y-up and swaps the placeholder material for the surface material', async () => {
    const model = await parseModel(ModelFormat.Obj, textData(OBJ))

    expect(model.upAxis).toBe(UpAxis.Y)
    expect(model.object).toBeInstanceOf(Group)
    const meshes: Mesh[] = []
    model.object.traverse((child) => { if (isMesh(child)) meshes.push(child) })
    expect(meshes).toHaveLength(1)
    expect(meshes[0]!.material).toBeInstanceOf(MeshStandardMaterial)
    expect(measureModel(model.object).triangles).toBe(2)
  })

  it('reads a PLY with faces as a mesh, computing the normals it lacks', async () => {
    const model = await parseModel(ModelFormat.Ply, textData(PLY_MESH))

    const mesh = onlyMesh(model.object)
    expect(mesh.geometry.hasAttribute('normal')).toBe(true)
    expect(measureModel(mesh).triangles).toBe(1)
  })

  it('reads a PLY without faces as a coloured point cloud sized to its extent', async () => {
    const model = await parseModel(ModelFormat.Ply, textData(PLY_POINTS))

    expect(model.object).toBeInstanceOf(Points)
    const material = (model.object as Points).material as PointsMaterial
    expect(material.vertexColors).toBe(true)
    // The cloud's diagonal is 5 (a 3-4-5 triangle).
    expect(material.size).toBeCloseTo(5 / 500)
    expect(measureModel(model.object)).toMatchObject({ triangles: 0, points: 2 })
  })

  it('reads a 3MF as Z-up', async () => {
    const model = await parseModel(ModelFormat.ThreeMf, threeMf())

    expect(model.upAxis).toBe(UpAxis.Z)
    const { triangles, size } = measureModel(model.object)
    expect(triangles).toBe(2)
    expect(size.toArray()).toEqual([10, 20, 30])
  })

  it('reads a glTF whose buffer is embedded as a data URI', async () => {
    const model = await parseModel(ModelFormat.Gltf, embeddedGltf())

    expect(model.upAxis).toBe(UpAxis.Y)
    expect(model.skippedFiles).toEqual([])
    expect(measureModel(model.object).triangles).toBe(1)
    expect(measureModel(model.object).size.toArray()).toEqual([1, 2, 0])
  })

  it('reads a GLB from its binary chunk', async () => {
    const model = await parseModel(ModelFormat.Glb, glb())

    expect(measureModel(model.object).triangles).toBe(1)
  })

  it('refuses a glTF that references a separate buffer file instead of fetching it', async () => {
    const fetchSpy = vi.spyOn(globalThis, 'fetch')
    const data = textData(JSON.stringify(gltfJson({ uri: 'scene.bin' })))

    await expect(parseModel(ModelFormat.Gltf, data)).rejects.toThrow(
      'This model references a separate file, "scene.bin", which the viewer cannot load.',
    )
    expect(fetchSpy).not.toHaveBeenCalled()
    fetchSpy.mockRestore()
  })

  it('shows a glTF without a separate texture file it references, and reports the file', async () => {
    // three logs the texture it could not load and carries on without it.
    const consoleError = vi.spyOn(console, 'error').mockImplementation(() => {})
    const uri = `data:application/octet-stream;base64,${btoa(String.fromCharCode(...Array.from(TRIANGLE_BYTES)))}`
    const data = textData(JSON.stringify(gltfJson({ uri }, {
      meshes: [{ primitives: [{ attributes: { POSITION: 0 }, material: 0 }] }],
      materials: [{ pbrMetallicRoughness: { baseColorTexture: { index: 0 } } }],
      textures: [{ source: 0 }],
      images: [{ uri: 'wood.png' }],
    })))

    const model = await parseModel(ModelFormat.Gltf, data)

    expect(model.skippedFiles).toEqual(['wood.png'])
    expect(measureModel(model.object).triangles).toBe(1)
    consoleError.mockRestore()
  })

  it('rejects malformed data', async () => {
    await expect(parseModel(ModelFormat.Gltf, textData('{ not json'))).rejects.toThrow()
  })

  it('rejects a file with no geometry to draw', async () => {
    // Longer than a binary STL header, which the loader probes before reading ASCII.
    await expect(parseModel(ModelFormat.Stl, textData(`solid ${'x'.repeat(100)}\nendsolid\n`)))
      .rejects.toThrow('The file contains no geometry to show.')
  })
})

describe('disposeModel', () => {
  it('disposes geometry, materials and their textures', () => {
    const geometry = new BufferGeometry()
    geometry.setAttribute('position', new Float32BufferAttribute([0, 0, 0, 1, 0, 0, 0, 1, 0], 3))
    const map = new Texture()
    const material = new MeshStandardMaterial({ map })
    const group = new Group()
    group.add(new Mesh(geometry, [material]))
    const disposals = [vi.spyOn(geometry, 'dispose'), vi.spyOn(material, 'dispose'), vi.spyOn(map, 'dispose')]

    disposeModel(group)

    for (const dispose of disposals) expect(dispose).toHaveBeenCalledOnce()
  })
})
