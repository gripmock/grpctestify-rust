import { lazy, Suspense } from 'react';
import type { EditorProps } from '@monaco-editor/react';

const Editor = lazy(async () => {
  const [{ default: EditorComponent, loader }, monaco] = await Promise.all([
    import('@monaco-editor/react'),
    import('monaco-editor'),
  ]);
  loader.config({ monaco });
  return { default: EditorComponent };
});

function EditorFallback({ height }: { height?: EditorProps['height'] }) {
  return (
    <div className="empty-state" style={{ height }}>
      Loading editor…
    </div>
  );
}

export function MonacoEditor(props: EditorProps) {
  return (
    <Suspense fallback={<EditorFallback height={props.height} />}>
      <Editor
        {...props}
        options={{
          fixedOverflowWidgets: true,
          ...props.options,
        }}
      />
    </Suspense>
  );
}
