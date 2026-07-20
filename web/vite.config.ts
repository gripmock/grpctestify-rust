import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import monacoEditorPluginModule from 'vite-plugin-monaco-editor'

const monacoEditorPlugin = (monacoEditorPluginModule as any).default || monacoEditorPluginModule

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
    monacoEditorPlugin({
      languageWorkers: ['editorWorkerService', 'json'],
    }),
  ],
  base: '/',
  build: {
    outDir: 'dist',
    chunkSizeWarningLimit: 500,
    rollupOptions: {
      output: {
        manualChunks(id) {
          // NOTE: do NOT force monaco-editor into a single manualChunk. Doing so
          // makes rolldown host the shared __vitePreload runtime helper inside
          // that (huge) chunk, and the entry then statically imports the helper —
          // dragging all ~4MB of monaco back onto the startup path. Left alone,
          // monaco is reached only through the dynamic import() in
          // src/components/MonacoEditor.tsx, so it is code-split into async chunks
          // (editor core + per-language grammars + workers) that load on demand.
          if (id.includes('node_modules/react-dom') || id.includes('node_modules/react/')) return 'react-vendor';
          if (id.includes('node_modules/lucide-react')) return 'lucide';
          if (id.includes('node_modules/zustand')) return 'zustand';
          if (id.includes('node_modules/lodash-es')) return 'lodash';
        },
      },
    },
  },
})
