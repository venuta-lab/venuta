import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { defineConfig, type Plugin } from 'vite'
import { createReadStream, existsSync } from 'node:fs'
import { join, normalize } from 'node:path'

// Serves app/dev-data at /_dev during `vite dev` only, so the UI can be previewed in a plain
// browser with a real library snapshot. Never part of a build: that folder holds the
// developer's own library data and cover art, and is git-ignored.
function devData(): Plugin {
  const dir = join(__dirname, 'dev-data')
  return {
    name: 'dev-data',
    apply: 'serve',
    configureServer(server) {
      server.middlewares.use('/_dev', (req, res, next) => {
        const file = normalize(join(dir, decodeURIComponent((req.url ?? '').split('?')[0])))
        if (!file.startsWith(dir) || !existsSync(file)) return next()
        res.setHeader('Content-Type', file.endsWith('.json') ? 'application/json' : 'image/jpeg')
        createReadStream(file).pipe(res)
      })
    },
  }
}

// Tauri expects a fixed port and must see Rust compile errors, so don't clear the screen.
export default defineConfig({
  plugins: [react(), tailwindcss(), devData()],
  // One React instance, always: motion must share it or hooks break.
  resolve: { dedupe: ['react', 'react-dom'] },
  optimizeDeps: { include: ['react', 'react-dom', 'react-dom/client', 'motion/react'] },
  clearScreen: false,
  server: { port: 5173, strictPort: true, watch: { ignored: ['**/dev-data/**', '**/src-tauri/**'] } },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: { target: 'es2022' },
})
