import {
  Box3,
  DoubleSide,
  Line,
  LoadingManager,
  Mesh,
  MeshStandardMaterial,
  Points,
  PointsMaterial,
  Texture,
  Vector3,
  type BufferGeometry,
  type Material,
  type Object3D,
} from 'three'
import { GLTFLoader } from 'three/addons/loaders/GLTFLoader.js'
import { OBJLoader } from 'three/addons/loaders/OBJLoader.js'
import { PLYLoader } from 'three/addons/loaders/PLYLoader.js'
import { STLLoader } from 'three/addons/loaders/STLLoader.js'
import { ThreeMFLoader } from 'three/addons/loaders/3MFLoader.js'
import { ModelFormat } from '../../shared/fileContentKind'

export enum UpAxis {
  Y = 'y',
  Z = 'z',
}

export interface LoadedModel {
  /** The model in the file's own coordinates. */
  object: Object3D
  /** 3D-printing formats are Z-up; the rest follow three.js and glTF in being Y-up. */
  upAxis: UpAxis
  /** Separate files the model references (textures) that were left out: the viewer reads one file. */
  skippedFiles: string[]
}

export interface ModelMeasurements {
  triangles: number
  points: number
  /** Bounding-box extent along the file's own X, Y and Z axes, in the file's units. */
  size: Vector3
}

const SURFACE_COLOR = 0xc5cad3

/** For formats that carry geometry but no material of their own. */
function surfaceMaterial(geometry: BufferGeometry): MeshStandardMaterial {
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

type Drawable = Mesh | Points | Line

// `instanceof` alone narrows to the classes' `any` generics; these pin the defaults.
export function isMesh(object: Object3D): object is Mesh {
  return object instanceof Mesh
}

function isPoints(object: Object3D): object is Points {
  return object instanceof Points
}

function isDrawable(object: Object3D): object is Drawable {
  return isMesh(object) || isPoints(object) || object instanceof Line
}

export function materialsOf(object: Drawable): Material[] {
  return Array.isArray(object.material) ? object.material : [object.material]
}

function parseStl(data: ArrayBuffer): LoadedModel {
  const geometry = new STLLoader().parse(data)
  // Stored facet normals are often zero or stale: exporters leave them out and slicers
  // ignore them. Derive them from the winding instead (flat, since STL is unindexed).
  geometry.computeVertexNormals()
  return { object: new Mesh(geometry, surfaceMaterial(geometry)), upAxis: UpAxis.Z, skippedFiles: [] }
}

function parseObj(data: ArrayBuffer): LoadedModel {
  const object = new OBJLoader().parse(new TextDecoder().decode(data))
  // OBJLoader leaves its placeholder material on every mesh (it does not follow `mtllib`).
  object.traverse((child) => {
    if (!isMesh(child)) return
    for (const material of materialsOf(child)) material.dispose()
    child.material = surfaceMaterial(child.geometry)
  })
  return { object, upAxis: UpAxis.Y, skippedFiles: [] }
}

function parsePly(data: ArrayBuffer): LoadedModel {
  const geometry = new PLYLoader().parse(data)
  if (geometry.index === null) {
    // No faces: a scanned point cloud.
    const hasColors = geometry.hasAttribute('color')
    const material = new PointsMaterial({ vertexColors: hasColors, color: hasColors ? 0xffffff : SURFACE_COLOR })
    const cloud = new Points(geometry, material)
    // Size the points to the cloud so they read at any scale.
    material.size = new Box3().setFromObject(cloud).getSize(new Vector3()).length() / 500
    return { object: cloud, upAxis: UpAxis.Y, skippedFiles: [] }
  }
  if (!geometry.hasAttribute('normal')) geometry.computeVertexNormals()
  return { object: new Mesh(geometry, surfaceMaterial(geometry)), upAxis: UpAxis.Y, skippedFiles: [] }
}

function parse3mf(data: ArrayBuffer): LoadedModel {
  return { object: new ThreeMFLoader().parse(data), upAxis: UpAxis.Z, skippedFiles: [] }
}

function parseGltf(data: ArrayBuffer): Promise<LoadedModel> {
  // A file opened from a workspace has no URL base, so a relative reference would be
  // fetched from the app's own origin. Embedded data passes; a separate file is refused.
  // A refused buffer fails the parse; a refused texture is dropped and reported.
  const skippedFiles = new Set<string>()
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
      resolve({ object: gltf.scene, upAxis: UpAxis.Y, skippedFiles: Array.from(skippedFiles) })
    }, reject)
  })
}

const PARSERS: Record<ModelFormat, (data: ArrayBuffer) => LoadedModel | Promise<LoadedModel>> = {
  [ModelFormat.Stl]: parseStl,
  [ModelFormat.Obj]: parseObj,
  [ModelFormat.Ply]: parsePly,
  [ModelFormat.ThreeMf]: parse3mf,
  [ModelFormat.Glb]: parseGltf,
  [ModelFormat.Gltf]: parseGltf,
}

/** Parses a model file. Rejects on malformed data, and on a file with nothing to draw. */
export async function parseModel(format: ModelFormat, data: ArrayBuffer): Promise<LoadedModel> {
  const model = await PARSERS[format](data)
  if (new Box3().setFromObject(model.object).isEmpty()) {
    disposeModel(model.object)
    throw new Error('The file contains no geometry to show.')
  }
  return model
}

export function measureModel(object: Object3D): ModelMeasurements {
  let triangles = 0
  let points = 0
  object.traverse((child) => {
    if (isMesh(child)) {
      triangles += (child.geometry.index ?? child.geometry.getAttribute('position')).count / 3
    } else if (isPoints(child)) {
      points += child.geometry.getAttribute('position').count
    }
  })
  return { triangles, points, size: new Box3().setFromObject(object).getSize(new Vector3()) }
}

/** Frees the GPU-side resources of everything `parseModel` built. */
export function disposeModel(object: Object3D): void {
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
