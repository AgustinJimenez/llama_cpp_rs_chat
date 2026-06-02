import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';
import checker from 'vite-plugin-checker';
import path from 'path';

export default defineConfig({
  plugins: [
    react(),
    // TypeScript + ESLint checking in dev — errors/warnings appear in browser overlay
    // and terminal so they're caught before commit. ESLint is disabled for production
    // builds because lint warnings block Tauri installer builds.
    checker({
      typescript: true,
      ...(process.env.NODE_ENV !== 'production' ? {
        eslint: {
          lintCommand: "eslint './src/**/*.{ts,tsx}' --rule 'i18next/no-literal-string: off'",
          useFlatConfig: false,
        },
      } : {}),
      overlay: { initialIsOpen: 'error' },
      enableBuild: process.env.NODE_ENV === 'production',
    }),
  ],
  clearScreen: false,
  server: {
    port: 14000,
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
        '**/deps/**',
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
        target: 'http://localhost:18080',
        changeOrigin: true,
      },
      '/ws': {
        target: 'http://localhost:18080',
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
    rollupOptions: {
      output: {
        manualChunks: {
          'vendor-react': ['react', 'react-dom'],
          'vendor-markdown': ['react-markdown', 'remark-gfm', 'react-syntax-highlighter'],
          'vendor-ui': ['@radix-ui/react-dialog', '@radix-ui/react-select', '@radix-ui/react-slider'],
        },
      },
    },
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  optimizeDeps: {
    exclude: ['deps'],
  },
});