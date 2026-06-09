// Rejects if `promise` does not settle within `ms`. Used to backstop event-driven promises that
// settle only inside a stream-event callback — so a lost event surfaces as a loud error rather
// than an endless spinner.
export function withTimeout<T>(promise: Promise<T>, ms: number, label: string, onTimeout?: () => void): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      onTimeout?.()
      reject(new Error(`${label} timed out after ${String(ms)}ms`))
    }, ms)
    promise.then(
      (value) => { clearTimeout(timer); resolve(value); },
      (error: unknown) => { clearTimeout(timer); reject(error instanceof Error ? error : new Error(String(error))); },
    )
  })
}
