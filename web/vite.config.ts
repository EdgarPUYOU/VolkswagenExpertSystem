import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import path from 'node:path'
import { defineConfig } from 'vite'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': path.resolve(import.meta.dirname, './src') },
  },
  server: {
    // In development the Rust API runs on :8000; Vite serves the UI on :5173.
    proxy: { '/api': 'http://localhost:8000', '/health': 'http://localhost:8000' },
  },
})
