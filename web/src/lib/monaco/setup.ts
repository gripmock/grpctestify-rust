let registered = false;


export function setupGctfLanguage(monaco: any) {
  if (registered) return;
  registered = true;

  const { languages } = monaco;

  languages.register({ id: 'gctf', extensions: ['.gctf'], aliases: ['GCTF'] });

  languages.setMonarchTokensProvider('gctf', {
    defaultToken: '',
    tokenPostfix: '.gctf',
    tokenizer: {
      root: [
        [/^---\s*/, 'delimiter', '@sectionName'],
        [/^#.*$/, 'comment'],
        [/#.*$/, 'comment'],
        [/"(?:[^"\\]|\\.)*"/, 'string'],
        [/\b\d+\.?\d*\b/, 'number'],
        [/@[\w.]+(?=\()/, { token: 'function', next: '@pluginArgs' }],
        [/@[\w.]+/, 'function'],
        [/\{\{\s*\w+\s*\}\}/, 'variable'],
        [/\$\w+/, 'variable'],
        [/\.\w+/, 'member'],
        [/==|!=|>=|<=|>|<|:=/, 'operator'],
        [/\band\b|\bor\b|\bxor\b|\bnot\b/, 'keyword.operator'],
        [/\bcontains\b|\bstartsWith\b|\bendsWith\b|\bmatches\b/, 'keyword.operator'],
        [/\btrue\b|\bfalse\b|\bnull\b/, 'keyword'],
        [/[{}[\](),:]/, '@brackets'],
        [/:\s/, 'delimiter'],
      ],
      sectionName: [
        [/[A-Z][A-Z_]*/, { token: 'keyword', next: '@pop' }],
        [/---/, { token: 'delimiter', next: '@pop' }],
        [/./, { token: '', next: '@pop' }],
      ],
      pluginArgs: [
        [/\(/, { token: '@brackets', next: '@pluginArgs' }],
        [/\)/, { token: '@brackets', next: '@pop' }],
        [/"/, 'string', '@pluginString'],
        [/[^()"]+/, ''],
      ],
      pluginString: [
        [/[^"\\]*(?:\\.[^"\\]*)*"/, 'string', '@pop' ],
        [/./, 'string'],
      ],
    },
  });

  languages.registerCompletionItemProvider('gctf', {
    triggerCharacters: ['-', '@', '.', ' ', '\n'],
    provideCompletionItems: (model: any, position: any) => {
      const range = {
        startLineNumber: position.lineNumber,
        endLineNumber: position.lineNumber,
        startColumn: position.column,
        endColumn: position.column,
      };
      const currentLine = model.getLineContent(position.lineNumber).trim();
      const suggestions: any[] = [];

      if (currentLine === '' || /^---\s*\w*$/.test(currentLine) || currentLine.match(/^---\s*$/)) {
        const sections = [
          { label: 'ADDRESS', detail: 'Server address (host:port)' },
          { label: 'ENDPOINT', detail: 'gRPC endpoint' },
          { label: 'REQUEST', detail: 'JSON request body' },
          { label: 'RESPONSE', detail: 'Expected response' },
          { label: 'ERROR', detail: 'Expected error' },
          { label: 'REQUEST_HEADERS', detail: 'Request metadata' },
          { label: 'ASSERTS', detail: 'Assertion rules' },
          { label: 'EXTRACT', detail: 'Variable extraction' },
          { label: 'META', detail: 'Test metadata' },
          { label: 'OPTIONS', detail: 'Runtime options' },
          { label: 'TLS', detail: 'TLS config' },
          { label: 'PROTO', detail: 'Proto descriptor' },
          { label: 'BENCH', detail: 'Benchmark config' },
        ];
        for (const s of sections) {
          suggestions.push({
            label: s.label,
            kind: languages.CompletionItemKind.Keyword,
            detail: s.detail,
            insertText: `--- ${s.label} ---\n`,
            range,
            insertTextRules: languages.CompletionItemInsertTextRule.InsertAsSnippet,
          });
        }
      }

      if (/(?:^|\s)(?:==|!=|>|<|>=|<=|\.\w+)\s*$/.test(currentLine) && !currentLine.includes('==') && !currentLine.includes('!=') && !currentLine.includes('>')) {
        const ops = ['==', '!=', '>', '<', '>=', '<=', 'contains', 'startsWith', 'endsWith', 'matches'];
        for (const op of ops) {
          suggestions.push({
            label: op,
            kind: languages.CompletionItemKind.Operator,
            detail: `Operator: ${op}`,
            insertText: ` ${op} `,
            range,
          });
        }
      }

      if (currentLine.includes('@')) {
        const plugins = [
          '@is_uuid', '@is_email', '@is_ip', '@is_url', '@is_timestamp',
          '@is_empty', '@is_base64', '@is_json', '@has_value',
          '@header()', '@trailer()', '@len()', '@env()', '@regex()',
          '@schema()',
          '@elapsed_ms', '@total_elapsed_ms', '@scope.message_count', '@scope.index',
          '@string.length', '@string.upper', '@string.lower',
          '@number.abs', '@number.floor', '@number.ceil',
          '@array.length', '@array.join', '@array.sort', '@array.unique',
          '@object.keys', '@object.values',
        ];
        for (const p of plugins) {
          suggestions.push({
            label: p,
            kind: languages.CompletionItemKind.Function,
            detail: 'Assertion plugin',
            insertText: p,
            range,
          });
        }
      }

      return { suggestions };
    },
  });

  languages.registerHoverProvider('gctf', {
    provideHover: (model: any, position: any) => {
      const word = model.getWordAtPosition(position);
      if (!word) return null;

      const docs: Record<string, string> = {
        'ADDRESS': '**ADDRESS** — Server address in `host:port` format.',
        'ENDPOINT': '**ENDPOINT** — gRPC method in `package.Service/Method`.',
        'REQUEST': '**REQUEST** — JSON request body.',
        'RESPONSE': '**RESPONSE** — Expected JSON response.',
        'ERROR': '**ERROR** — Expected gRPC error.',
        'ASSERTS': '**ASSERTS** — Assertion expressions evaluated against response.',
        'EXTRACT': '**EXTRACT** — Extract values from response into `{{variable}}` using JQ.',
        'META': '**META** — Metadata: name, summary, tags, owner.',
        '@is_uuid': '`@is_uuid(value)` — Checks if value is a valid UUID.',
        '@is_email': '`@is_email(value)` — Valid email address check.',
        '@schema': '`@schema(instance, schema)` — JSON Schema validation.',
        '@header': '`@header("name")` — Extract a response header value.',
        '@len': '`@len(value)` — Length of string/array/object.',
      };

      const doc = docs[word.word];
      if (doc) {
        return {
          contents: [{ value: doc }],
          range: {
            startLineNumber: position.lineNumber, endLineNumber: position.lineNumber,
            startColumn: word.startColumn, endColumn: word.endColumn,
          },
        };
      }

      const varMatch = word.word.match(/^\{\{(\w+)\}\}$/);
      if (varMatch) {
        return {
          contents: [{ value: `**Variable**: \`${varMatch[1]}\` — defined in EXTRACT section.` }],
        };
      }

      return null;
    },
  });
}
