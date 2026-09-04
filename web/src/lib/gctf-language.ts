import { useStore } from './store';
import { draftFileName } from './http-endpoint';

const LANGUAGE_ID = 'gctf';
const registered = new WeakSet<object>();

type Monaco = any;

export function ensureGctfLanguage(monaco: Monaco) {
  if (registered.has(monaco)) return;
  registered.add(monaco);

  if (!monaco.languages.getLanguages().some((l: any) => l.id === LANGUAGE_ID)) {
    monaco.languages.register({ id: LANGUAGE_ID });
  }

  monaco.languages.setMonarchTokensProvider(LANGUAGE_ID, {
    tokenizer: {
      root: [
        [/^---\s+[A-Z_]+(\s+[^-\n][^\n]*?)?\s+---\s*$/, 'keyword'],
        [/^\s*(\/\/|#).*$/, 'comment'],
        [/\{\{[^}]*\}\}/, 'variable'],
        [/@[a-zA-Z_][a-zA-Z0-9_.]*/, 'function'],
        [/"[^"]*"/, 'string'],
        [/\b\d+(\.\d+)?\b/, 'number'],
      ],
    },
  });

  monaco.languages.registerCompletionItemProvider(LANGUAGE_ID, {
    triggerCharacters: ['-', '@', '{', '.', ':'],
    provideCompletionItems: async (model: any, position: any) => {
      const items = await post('/api/complete', model, position);
      if (!Array.isArray(items)) return { suggestions: [] };
      const word = model.getWordUntilPosition(position);
      const range = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: word.startColumn,
        endColumn: word.endColumn,
      };
      return {
        suggestions: items.map((c: any) => ({
          label: c.label,
          kind: completionKind(monaco, c.kind),
          detail: c.detail ?? undefined,
          documentation: docText(c.documentation),
          insertText: c.insertText ?? c.insert_text ?? c.label,
          insertTextRules:
            (c.insertTextFormat ?? c.insert_text_format) === 2
              ? monaco.languages.CompletionItemInsertTextRule.InsertAsSnippet
              : undefined,
          range,
        })),
      };
    },
  });

  monaco.languages.registerHoverProvider(LANGUAGE_ID, {
    provideHover: async (model: any, position: any) => {
      const data = await post('/api/hover', model, position);
      const hover = data?.hover;
      if (!hover) return null;
      const contents = hoverText(hover.contents);
      if (!contents.length) return null;
      return { contents: contents.map(value => ({ value })) };
    },
  });
}

async function post(url: string, model: any, position: any): Promise<any> {
  try {
    const res = await fetch(url, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        content: model.getValue(),
        file_name: editedFileName(),
        line: position.lineNumber - 1,
        character: position.column - 1,
      }),
    });
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

function completionKind(monaco: Monaco, kind: number | undefined) {
  const K = monaco.languages.CompletionItemKind;
  switch (kind) {
    case 3: return K.Function;
    case 6: return K.Variable;
    case 14: return K.Keyword;
    case 15: return K.Snippet;
    case 10: return K.Property;
    case 12: return K.Value;
    default: return K.Text;
  }
}

function docText(doc: any): string | undefined {
  if (!doc) return undefined;
  if (typeof doc === 'string') return doc;
  if (typeof doc.value === 'string') return doc.value;
  return undefined;
}

function hoverText(contents: any): string[] {
  if (!contents) return [];
  if (typeof contents === 'string') return [contents];
  if (Array.isArray(contents)) return contents.flatMap(hoverText);
  if (typeof contents.value === 'string') return [contents.value];
  return [];
}

export { LANGUAGE_ID as GCTF_LANGUAGE };

function editedFileName(): string {
  const st = useStore.getState();
  return st.workspacePath ?? draftFileName(st.workspacePath, st.request.endpoint);
}
