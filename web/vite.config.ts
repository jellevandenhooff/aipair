import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// Dev server uses different ports to avoid conflict with production
// Backend: 3001 (dev) vs 3000 (prod)
// Frontend: 5174 (dev) vs 5173 (prod, if applicable)
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5174,
    proxy: {
      '/api': {
        target: 'http://localhost:3001',
        changeOrigin: true,
      },
    },
  },
})
