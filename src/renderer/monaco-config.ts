import { loader } from '@monaco-editor/react'
import * as monaco from 'monaco-editor'

// Configure Monaco to use the locally installed package instead of CDN
loader.config({ monaco })
