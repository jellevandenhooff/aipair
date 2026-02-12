import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

const apiPort = process.env.AIPAIR_PORT || '3000'

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': {
        target: `http://localhost:${apiPort}`,
        changeOrigin: true,
      },
    },
  },
})
