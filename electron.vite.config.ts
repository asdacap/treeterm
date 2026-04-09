import { defineConfig, externalizeDepsPlugin } from 'electron-vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'path'
import { copyFileSync, mkdirSync } from 'fs'

const copyLoadingHtmlPlugin = () => ({
  name: 'copy-loading-html',
  closeBundle() {
    const srcPath = resolve(__dirname, 'src/main/loading.html')
    const destDir = resolve(__dirname, 'out/main')
    const destPath = resolve(destDir, 'loading.html')
    
    try {
      mkdirSync(destDir, { recursive: true })
      copyFileSync(srcPath, destPath)
      console.log('Copied loading.html to out/main/')
    } catch (error) {
      console.error('Failed to copy loading.html:', error)
    }
  }
})

export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin(), copyLoadingHtmlPlugin()]
  },
  preload: {
    plugins: [externalizeDepsPlugin()]
  },
  renderer: {
    plugins: [react()],
    build: {
      rollupOptions: {
        output: {
          manualChunks: {
            monaco: ['monaco-editor']
          }
        }
      }
    }
  }
})
