import React, { useEffect, useState } from 'react'
import { base64ToBytes } from '../utils/base64'

interface PdfViewerProps {
  /** The PDF's bytes, base64-encoded as a read returns them. */
  base64: string
  /** Names the frame for assistive tech. */
  fileName: string
}

/**
 * Shows a PDF in Chromium's built-in viewer, which brings page navigation, zoom and print.
 * The bytes go through a blob URL rather than a data: URL, which Chromium caps at 2MB;
 * the blob is released when the content changes or the viewer unmounts.
 */
export function PdfViewer({ base64, fileName }: PdfViewerProps): React.JSX.Element {
  const [frame, setFrame] = useState<HTMLIFrameElement | null>(null)

  useEffect(() => {
    if (!frame) return
    const url = URL.createObjectURL(new Blob([base64ToBytes(base64)], { type: 'application/pdf' }))
    frame.src = url
    return () => { URL.revokeObjectURL(url) }
  }, [frame, base64])

  return <iframe ref={setFrame} className="pdf-viewer" title={fileName} />
}
