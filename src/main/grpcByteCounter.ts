/**
 * Counts bytes received from the daemon across every RPC of a connection.
 *
 * Every response message — unary or streamed — passes through its method's
 * `responseDeserialize(buffer)`, so wrapping those is the one place that sees
 * the raw size of all traffic. Interceptors only see decoded messages.
 */

import * as grpc from '@grpc/grpc-js'
import { TreeTermDaemonService, type TreeTermDaemonClient } from '../generated/treeterm'

export class ByteCounter {
  private _total = 0

  add(bytes: number): void {
    this._total += bytes
  }

  get total(): number {
    return this._total
  }
}

interface MethodDefinition {
  responseDeserialize: (value: Buffer) => unknown
}

/** Copy of `service` whose response deserializers add each message's size to `counter`. */
export function countingService<S extends Record<string, MethodDefinition>>(service: S, counter: ByteCounter): S {
  const wrapped: Record<string, MethodDefinition> = {}
  for (const [name, method] of Object.entries(service)) {
    wrapped[name] = {
      ...method,
      responseDeserialize: (value: Buffer): unknown => {
        counter.add(value.length)
        return method.responseDeserialize(value)
      },
    }
  }
  return wrapped as S
}

export function createCountingClient(
  address: string,
  credentials: grpc.ChannelCredentials,
  options: Partial<grpc.ClientOptions>,
  counter: ByteCounter,
): TreeTermDaemonClient {
  // Same construction as the generated `TreeTermDaemonClient`, over the counting definition.
  const Client = grpc.makeGenericClientConstructor(countingService(TreeTermDaemonService, counter), 'treeterm.TreeTermDaemon')
  return new Client(address, credentials, options) as unknown as TreeTermDaemonClient
}
