import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import checker from 'vite-plugin-checker';
import path from 'path';

export default defineConfig({
  plugins: [
    react(),
    // Only run TS/ESLint checker during build — dev checking is handled by VS Code.
    // Running checker workers in dev leaks memory over long sessions (known issue).
    ...(process.env.NODE_ENV === 'production' ? [checker({
      typescript: true,
      eslint: {
        lintCommand: 'eslint "src/**/*.{ts,tsx}"',
        useFlatConfig: false,
      },
      overlay: { initialIsOpen: 'error' },
      enableBuild: true,
    })] : []),
  ],
  clearScreen: false,
  server: {
    port: 4000,
    strictPort: true,
    host: true,
    watch: {
      // Exclude large/irrelevant directories to prevent Windows file watcher memory leak
      // (chokidar leaks file handles on Windows when watching rapidly-changing dirs)
      ignored: [
        '**/node_modules/**',
        '**/target/**',
        '**/src-tauri/target/**',
        '**/results/**',
        '**/expected/**',
        '**/php-8.2.30/**',
        '**/*.gguf',
        '**/*.db',
        '**/*.stackdump',
      ],
    },
    hmr: {
      path: '/__vite_hmr',
    },
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