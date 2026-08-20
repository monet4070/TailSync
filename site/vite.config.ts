import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  publicDir: resolve(import.meta.dirname, '../assets'),
  server: {
    port: 5174,
    strictPort: true,
    fs: {
      // Allow importing the theme manifests from ../themes during dev.
      allow: [resolve(import.meta.dirname, '..')],
    },
  },
  build: {
    target: 'esnext',
    rollupOptions: {
      input: {
        main: resolve(import.meta.dirname, 'index.html'),
        themes: resolve(import.meta.dirname, 'themes.html'),
      },
    },
  },
})
