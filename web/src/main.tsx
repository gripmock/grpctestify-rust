import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './index.css'
import App from './App.tsx'

// Note: monaco-editor is registered lazily (self-hosted, no CDN) the first
// time an editor mounts — see src/components/MonacoEditor.tsx — so the ~4MB
// monaco chunk is kept off the initial page-load path.

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
