import { sveltekit } from '@sveltejs/kit/vite'
import masterCSS from '@master/css.svelte/vite'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [sveltekit(), masterCSS({ mode: 'static' })],
  server: { fs: { allow: ['..'] } },
  test: { include: ['src/**/*.test.ts'] }
})
