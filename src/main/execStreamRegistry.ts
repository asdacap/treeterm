import { ExecEventType, type ExecEvent } from '../shared/ipc-types'

/**
 * Tracks in-flight exec streams with the window (sender) and connection that own them,
 * so a broken connection can settle its execs instead of leaving renderers waiting on
 * events that will never arrive.
 */
export interface ExecStreamEntry<TStream extends { cancel: () => void }, TSender> {
  stream: TStream
  sender: TSender
  connectionId: string
}

export interface ExecStreamRegistry<TStream extends { cancel: () => void }, TSender> {
  add: (execId: string, entry: ExecStreamEntry<TStream, TSender>) => void
  get: (execId: string) => ExecStreamEntry<TStream, TSender> | undefined
  has: (execId: string) => boolean
  delete: (execId: string) => void
  /**
   * Settle every in-flight exec on `connectionId`: cancel its stream (best effort),
   * notify its owning window with a terminal Error event, and drop the entry.
   */
  failAllForConnection: (
    connectionId: string,
    notify: (sender: TSender, execId: string, event: ExecEvent) => void,
  ) => void
}

export function createExecStreamRegistry<
  TStream extends { cancel: () => void },
  TSender,
>(): ExecStreamRegistry<TStream, TSender> {
  const entries = new Map<string, ExecStreamEntry<TStream, TSender>>()

  return {
    add: (execId, entry) => { entries.set(execId, entry) },
    get: (execId) => entries.get(execId),
    has: (execId) => entries.has(execId),
    delete: (execId) => { entries.delete(execId) },

    failAllForConnection: (connectionId, notify) => {
      for (const [execId, entry] of entries) {
        if (entry.connectionId !== connectionId) continue
        entries.delete(execId)
        try {
          entry.stream.cancel()
        } catch (error) {
          // The orphaned stream may already be dead — cancel is best-effort; the
          // notification below is what settles the renderer either way.
          console.error(`[exec] cancel failed for ${execId} during connection failure:`, error)
        }
        notify(entry.sender, execId, {
          type: ExecEventType.Error,
          message: `connection ${connectionId} lost while command was running`,
        })
      }
    },
  }
}
