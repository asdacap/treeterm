import React, { useEffect, useState } from 'react'
import { Box, Loader2, RotateCcw } from 'lucide-react'
import type { ModelFormat } from '../../shared/fileContentKind'
import { base64ToBytes } from '../utils/base64'
import { disposeModel, measureModel, parseModel, type LoadedModel } from '../lib/modelLoader'
import { createModelStage, type ModelStage } from '../lib/modelStage'

enum ParseStatus {
  Parsing = 'parsing',
  Ready = 'ready',
  Failed = 'failed',
}

type ParseState =
  | { status: ParseStatus.Parsing }
  | { status: ParseStatus.Ready; model: LoadedModel }
  | { status: ParseStatus.Failed; error: string }

enum StageStatus {
  Starting = 'starting',
  Running = 'running',
  Failed = 'failed',
}

type StageState =
  | { status: StageStatus.Starting }
  | { status: StageStatus.Running; stage: ModelStage }
  | { status: StageStatus.Failed; error: string }

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

function formatCount(count: number): string {
  return Math.round(count).toLocaleString()
}

function formatLength(length: number): string {
  return String(Number(length.toPrecision(4)))
}

function renderStageOverlay(stage: StageState, wireframe: boolean, onToggleWireframe: () => void): React.ReactNode {
  const views: Record<StageStatus, () => React.ReactNode> = {
    [StageStatus.Starting]: () => null,
    [StageStatus.Running]: () => stage.status === StageStatus.Running && (
      <div className="model-viewer-toolbar">
        <button
          className="file-viewer-toggle-btn"
          title="Frame the whole model"
          onClick={() => { stage.stage.resetView() }}
        >
          <RotateCcw size={12} /> Reset view
        </button>
        <button
          className={`file-viewer-toggle-btn${wireframe ? ' active' : ''}`}
          aria-pressed={wireframe}
          title="Show the mesh edges"
          onClick={onToggleWireframe}
        >
          <Box size={12} /> Wireframe
        </button>
      </div>
    ),
    [StageStatus.Failed]: () => stage.status === StageStatus.Failed && (
      <div className="file-viewer-error">{stage.error}</div>
    ),
  }
  return views[stage.status]()
}

function ModelScene({ model }: { model: LoadedModel }): React.JSX.Element {
  const [container, setContainer] = useState<HTMLDivElement | null>(null)
  const [stage, setStage] = useState<StageState>({ status: StageStatus.Starting })
  const [wireframe, setWireframe] = useState(false)

  useEffect(() => {
    if (!container) return
    let created: ModelStage
    try {
      created = createModelStage(container, model)
    } catch (err) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- the stage is a WebGL context bound to this DOM node; it can only be created once the node exists
      setStage({ status: StageStatus.Failed, error: `3D view unavailable: ${errorMessage(err)}` })
      return
    }
    setStage({ status: StageStatus.Running, stage: created })
    return () => { created.dispose() }
  }, [container, model])

  const { triangles, points, size } = measureModel(model.object)
  const counts = [
    triangles > 0 ? `${formatCount(triangles)} triangles` : '',
    points > 0 ? `${formatCount(points)} points` : '',
  ].filter(Boolean).join(' · ')

  const toggleWireframe = (): void => {
    if (stage.status !== StageStatus.Running) return
    stage.stage.setWireframe(!wireframe)
    setWireframe(!wireframe)
  }

  return (
    <div className="model-viewer">
      <div className="model-viewer-canvas" ref={setContainer} />
      {renderStageOverlay(stage, wireframe, toggleWireframe)}
      {model.skippedFiles.length > 0 && (
        <div className="model-viewer-warning">
          Shown without the separate files it references: {model.skippedFiles.join(', ')}
        </div>
      )}
      <div className="model-viewer-stats">
        {counts} · {formatLength(size.x)} × {formatLength(size.y)} × {formatLength(size.z)}
      </div>
      <div className="model-viewer-hint">Drag to orbit · Right-drag to pan · Scroll to zoom</div>
    </div>
  )
}

function renderParse(parse: ParseState): React.ReactNode {
  const views: Record<ParseStatus, () => React.ReactNode> = {
    [ParseStatus.Parsing]: () => (
      <div className="file-viewer-placeholder"><Loader2 size={14} className="spinning" /> Reading model…</div>
    ),
    [ParseStatus.Ready]: () => parse.status === ParseStatus.Ready && <ModelScene model={parse.model} />,
    [ParseStatus.Failed]: () => parse.status === ParseStatus.Failed && (
      <div className="file-viewer-error">{parse.error}</div>
    ),
  }
  return views[parse.status]()
}

interface ModelViewerProps {
  /** The model file's bytes, base64-encoded as a read returns them. */
  base64: string
  format: ModelFormat
}

/** Default export so FileViewer can lazy-load it: three.js stays out of the main bundle. */
export default function ModelViewer({ base64, format }: ModelViewerProps): React.JSX.Element {
  const [parse, setParse] = useState<ParseState>({ status: ParseStatus.Parsing })

  useEffect(() => {
    let cancelled = false
    let loaded: LoadedModel | null = null
    const load = async (): Promise<LoadedModel> => parseModel(format, base64ToBytes(base64).buffer)
    load().then((model) => {
      if (cancelled) {
        disposeModel(model.object)
        return
      }
      loaded = model
      setParse({ status: ParseStatus.Ready, model })
    }, (err: unknown) => {
      if (!cancelled) setParse({ status: ParseStatus.Failed, error: `Could not read model: ${errorMessage(err)}` })
    })
    return () => {
      cancelled = true
      if (loaded) disposeModel(loaded.object)
    }
  }, [base64, format])

  return <>{renderParse(parse)}</>
}
