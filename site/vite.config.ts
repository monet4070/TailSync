import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  publicDir: resolve(import.meta.dirname, '../assets'),
  server: {
    port: 5174,
    strictPort: true,
  },
  build: {
    target: 'esnext',
  },
})
