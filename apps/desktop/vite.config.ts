import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
// package.json holds the release version; tauri.conf.json reads it too.
import pkg from './package.json' with { type: 'json' };
export default defineConfig({ plugins: [svelte()], define: { __APP_VERSION__: JSON.stringify(pkg.version) }, clearScreen: false, server: { port: 1420, strictPort: true }, envPrefix: ['VITE_', 'TAURI_ENV_'], build: { target: ['es2022', 'safari15'] } });
