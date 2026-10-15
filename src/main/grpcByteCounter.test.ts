import { describe, it, expect } from 'vitest'
import * as grpc from '@grpc/grpc-js'
import { ByteCounter, countingService, createCountingClient } from './grpcByteCounter'
import { CreatePtyResponse, TreeTermDaemonService } from '../generated/treeterm'

describe('ByteCounter', () => {
  it('starts at zero and accumulates', () => {
    const counter = new ByteCounter()
    expect(counter.total).toBe(0)
    counter.add(10)
    counter.add(5)
    expect(counter.total).toBe(15)
  })
})

describe('countingService', () => {
  it('counts every response buffer and still decodes it', () => {
    const counter = new ByteCounter()
    const service = countingService(TreeTermDaemonService, counter)
    const encoded = Buffer.from(CreatePtyResponse.encode({ sessionId: 'pty-1' }).finish())

    expect(service.createPty.responseDeserialize(encoded)).toEqual({ sessionId: 'pty-1' })
    expect(service.createPty.responseDeserialize(encoded)).toEqual({ sessionId: 'pty-1' })
    expect(counter.total).toBe(encoded.length * 2)
  })

  it('wraps every method and keeps the rest of the definition', () => {
    const counter = new ByteCounter()
    const service = countingService(TreeTermDaemonService, counter)

    expect(Object.keys(service)).toEqual(Object.keys(TreeTermDaemonService))
    for (const [name, method] of Object.entries(service)) {
      const original = TreeTermDaemonService[name as keyof typeof TreeTermDaemonService]
      expect(method.path).toBe(original.path)
      expect(method.responseDeserialize).not.toBe(original.responseDeserialize)
    }
  })
})

describe('createCountingClient', () => {
  it('builds a daemon client whose service counts responses', () => {
    const counter = new ByteCounter()
    const client = createCountingClient('unix:///tmp/none.sock', grpc.credentials.createInsecure(), {}, counter)
    try {
      expect(typeof client.createPty).toBe('function')
      const service = (client.constructor as unknown as { service: typeof TreeTermDaemonService }).service
      service.createPty.responseDeserialize(Buffer.from(CreatePtyResponse.encode({ sessionId: 'x' }).finish()))
      expect(counter.total).toBeGreaterThan(0)
    } finally {
      client.close()
    }
  })
})
