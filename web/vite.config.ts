import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { VitePWA } from 'vite-plugin-pwa'

// The Rust launcher proxies nothing in dev: run `cargo run -- --no-browser`
// and Vite forwards /api to it.
export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      registerType: 'autoUpdate',
      injectRegister: 'auto',
      manifest: {
        name: 'piShop',
        short_name: 'piShop',
        description: 'piShop para Steam Deck',
        display: 'fullscreen',
        orientation: 'landscape',
        background_color: '#0b0d12',
        theme_color: '#0b0d12',
        icons: [{ src: 'icon.svg', sizes: 'any', type: 'image/svg+xml', purpose: 'any' }],
      },
      workbox: {
        globPatterns: ['**/*.{js,css,html,svg,woff2}'],
        navigateFallbackDenylist: [/^\/api\//],
        maximumFileSizeToCacheInBytes: 8 * 1024 * 1024,
      },
    }),
  ],
  server: { proxy: { '/api': 'http://127.0.0.1:47800' } },
  build: { target: 'es2022', chunkSizeWarningLimit: 2048 },
})
