import blits from '@lightningjs/blits/vite'
import { defineConfig } from 'vite'

const classicBundle = {
  name: 'screenroom-tv-classic-bundle',
  renderChunk(code: string) {
    return code.includes('import.meta') ? code.replace(/import\.meta\.url/g, 'window.location.href') : code
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
    rollupOptions: {
      output: {
        entryFileNames: 'main.js',
        assetFileNames: 'assets/[name][extname]',

        inlineDynamicImports: true,
        manualChunks: undefined,
      },
    },
  },
  server: {
    port: 5175,
    host: '127.0.0.1',
  },
})
