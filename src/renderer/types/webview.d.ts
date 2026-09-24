import type { DetailedHTMLProps, HTMLAttributes } from 'react'

// Electron's <webview> guest tag (enabled via webPreferences.webviewTag in main).
declare global {
  namespace JSX {
    interface IntrinsicElements {
      webview: DetailedHTMLProps<HTMLAttributes<HTMLElement>, HTMLElement> & {
        src: string
        partition?: string
      }
    }
  }
}
