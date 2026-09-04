import { fileURLToPath } from 'node:url'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import monacoEditorPluginModule from 'vite-plugin-monaco-editor'

const monacoEditorPlugin = (monacoEditorPluginModule as any).default || monacoEditorPluginModule

/** Monaco ships language services for CSS, HTML and TypeScript, and the barrel
 *  imports all of them: 8.3 MB of web workers, compiled into every binary, for
 *  languages a `.gctf` file cannot contain. The editor keeps everything else —
 *  its contributions, its stylesheets, the basic-language grammars, and the
 *  JSON service the request bodies are checked with. */
const STUBBED_LANGUAGES = ['css', 'html', 'typescript']
const STUB_ID = '\0monaco-language-stub'

function dropMonacoLanguages() {
  const drop = new RegExp(`monaco-editor/esm/vs/languages/features/(${STUBBED_LANGUAGES.join('|')})/register\\.js$`)
  let dropped = 0
  return {
    name: 'drop-monaco-languages',
    enforce: 'pre' as const,
    async resolveId(this: any, source: string, importer: string | undefined, options: any) {
      if (source === STUB_ID) return STUB_ID
      const resolved = await this.resolve(source, importer, { ...options, skipSelf: true })
      if (resolved && drop.test(resolved.id.replace(/\\/g, '/'))) {
        dropped++
        return STUB_ID
      }
      return null
    },
    load(id: string) {
      return id === STUB_ID ? 'export {}' : null
    },
    buildEnd() {
      /* Monaco moves these paths between releases; a silent no-op would put the
         workers back and nothing would say so. */
      if (dropped < STUBBED_LANGUAGES.length) {
        throw new Error(
          `drop-monaco-languages matched ${dropped} of ${STUBBED_LANGUAGES.length} language services — ` +
          'monaco moved them, and the workers are back in the bundle',
        )
      }
    },
  }
}

export default defineConfig({
  resolve: {
    alias: { luvo: fileURLToPath(new URL('./luvo', import.meta.url)) },
  },
  plugins: [
    react(),
    tailwindcss(),
    dropMonacoLanguages(),
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
