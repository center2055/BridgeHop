import adapter from '@sveltejs/adapter-static';
import { vitePreprocess } from '@sveltejs/vite-plugin-svelte';

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    // Static SPA: a single index.html fallback handles client-side routing inside Tauri.
    adapter: adapter({ fallback: 'index.html' })
  }
};

export default config;
