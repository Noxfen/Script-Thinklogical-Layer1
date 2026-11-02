import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// Tauri: porta fissa, niente clear screen, build per WebView2 / WKWebView
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  build: { target: ['es2021', 'chrome105', 'safari15'], outDir: 'dist', emptyOutDir: true },
});
