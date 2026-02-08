import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import checker from 'vite-plugin-checker';
import path from 'path';

export default defineConfig({
  plugins: [
    react(),
    checker({
      typescript: true,
      eslint: {
        lintCommand: 'eslint "src/**/*.{ts,tsx}"',
        useFlatConfig: false,
      },
      overlay: { initialIsOpen: false },
    }),
  ],
  clearScreen: false,
  server: {
    port: 4000,
    strictPort: true,
    host: true,
    proxy: {
      '/api': {
        target: 'http://localhost:8000',
        changeOrigin: true,
      },
      '/ws': {
        target: 'http://localhost:8000',
        changeOrigin: true,
        ws: true, // Enable WebSocket proxying
        configure: (proxy) => {
          // Disable buffering for real-time streaming
          proxy.on('proxyReqWs', (proxyReq) => {
            proxyReq.setHeader('Connection', 'Upgrade');
          });
        },
      },
    },
  },
  envPrefix: ['VITE_', 'TAURI_'],
  build: {
    target: 'esnext',
    minify: !process.env.TAURI_DEBUG ? 'esbuild' : false,
    sourcemap: !!process.env.TAURI_DEBUG,
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
});