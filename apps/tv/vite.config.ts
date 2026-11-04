import blits from '@lightningjs/blits/vite'
import { defineConfig } from 'vite'

const classicBundle = {
  name: 'screenroom-tv-classic-bundle',
  renderChunk(code: string) {
    if (!code.includes('import.meta')) return code
    return code
      .replace(/import\.meta\.resolve/g, 'undefined')
      .replace(/import\.meta\.url/g, 'window.location.href')
  },
}

export default defineConfig({
  base: './',
  plugins: [...blits, classicBundle],
  build: {
    target: 'es2019',
    outDir: 'dist',
    emptyOutDir: true,

    assetsInlineLimit: Number.MAX_SAFE_INTEGER,
    cssCodeSplit: false,
    sourcemap: false,
    modulePreload: false,
    rolldownOptions: {
      output: {
        entryFileNames: 'main.js',
        assetFileNames: 'assets/[name][extname]',

        codeSplitting: false,
      },
    },
  },
  server: {
    port: 5175,
    host: '127.0.0.1',
  },
})
