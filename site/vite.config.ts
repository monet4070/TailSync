import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// The site quotes the product version in several places. Read it from
// package.json — which `scripts/bump-version.mjs` already rewrites on release —
// so a tag bump propagates into the copy instead of being hand-edited.
const { version } = JSON.parse(
  readFileSync(resolve(import.meta.dirname, 'package.json'), 'utf8'),
) as { version: string }

export default defineConfig({
  plugins: [react()],
  publicDir: resolve(import.meta.dirname, '../assets'),
  define: {
    __TAILSYNC_VERSION__: JSON.stringify(version),
  },
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
        preview: resolve(import.meta.dirname, 'preview.html'),
      },
    },
  },
})
