import { sveltekit } from '@sveltejs/kit/vite';
import tailwindcss from '@tailwindcss/vite';
import { defineConfig } from 'vite';

// Tauri injects this when developing against a physical mobile device.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [tailwindcss(), sveltekit()],
  // Tauri shows its own (Rust) errors, so keep the Vite output visible.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: 'ws', host, port: 1421 } : undefined,
    watch: {
      // Don't reload the dev server when Rust sources change.
      ignored: ['**/src-tauri/**']
    }
  }
});
