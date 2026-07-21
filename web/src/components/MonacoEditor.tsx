import { lazy, Suspense } from 'react';
import type { EditorProps } from '@monaco-editor/react';

// Lazily load monaco-editor and register the self-hosted instance with the
// loader, then return the Editor component. Keeping this behind a dynamic
// import splits the ~4MB monaco chunk out of the initial bundle — it is only
// fetched the first time an editor actually mounts.
const Editor = lazy(async () => {
  const [{ default: EditorComponent, loader }, monaco] = await Promise.all([
    import('@monaco-editor/react'),
    import('monaco-editor'),
  ]);
  // Register the self-hosted monaco (offline / strict-CSP: no CDN loader).
  // Runs before Editor mounts, so loader.init() picks up this instance.
  loader.config({ monaco });
  return { default: EditorComponent };
});

function EditorFallback({ height }: { height?: EditorProps['height'] }) {
  return (
    <div style={{
      height, display: 'flex', alignItems: 'center', justifyContent: 'center',
      fontSize: 12, color: 'var(--text-muted)', background: 'var(--bg-tertiary)',
    }}>
      Loading editor…
    </div>
  );
}

export function MonacoEditor(props: EditorProps) {
  return (
    <Suspense fallback={<EditorFallback height={props.height} />}>
      <Editor {...props} />
    </Suspense>
  );
}
