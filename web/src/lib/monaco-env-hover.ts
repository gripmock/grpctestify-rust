
export function registerEnvHoverProvider(
  monaco: any,
  getEnv: () => { name: string; variables: Record<string, string>; mutedVariables?: string[] } | null | undefined,
) {
  const langs = monaco.languages.getLanguages?.()?.map((l: any) => l.id) || ['json', 'plaintext'];

  for (const lang of langs) {
    monaco.languages.registerHoverProvider(lang, {
      provideHover(model: any, position: any) {
        const env = getEnv();
        if (!env?.variables) return null;

        const word = model.getWordAtPosition(position);
        if (!word) return null;

        const range = {
          startLineNumber: position.lineNumber,
          startColumn: word.startColumn,
          endLineNumber: position.lineNumber,
          endColumn: word.endColumn,
        };

        const wordText = model.getValueInRange(range);
        const match = wordText.match(/^\{\{(\w+)\}\}$/);
        if (!match) return null;

        const key = match[1];
        const val = env.variables[key];
        if (val === undefined) return null;

        const muted = (env.mutedVariables || []).includes(key);
        const lines = [
          `**\`{{${key}}}\`** → ` + (val ? `\`${val}\`` : '*empty (secret)*'),
          `from environment: **${env.name}**`,
        ];
        if (muted) lines.push('_muted — excluded from substitution_');

        return { contents: [{ value: lines.join('  \n') }] };
      },
    });
  }
}

export function addEnvDecorations(
  editor: any,
  monaco: any,
  getEnv: () => { variables: Record<string, string>; mutedVariables?: string[] } | null | undefined,
) {
  let collection: any = null;

  const updateDecorations = () => {
    const model = editor.getModel();
    if (!model) return;

    const text = model.getValue();
    const regex = /\{\{(\w+)\}\}/g;
    const decorations: any[] = [];
    let match: RegExpExecArray | null;

    while ((match = regex.exec(text)) !== null) {
      const key = match[1];
      const env = getEnv();
      const hasKey = env?.variables?.[key] !== undefined;
      const muted = hasKey && (env?.mutedVariables || []).includes(key);
      const isSecret = hasKey && !env?.variables[key];

      const startPos = model.getPositionAt(match.index);
      const endPos = model.getPositionAt(match.index + match[0].length);

      decorations.push({
        range: new monaco.Range(
          startPos.lineNumber, startPos.column,
          endPos.lineNumber, endPos.column,
        ),
        options: {
          inlineClassName: hasKey
            ? (muted ? 'env-var-muted' : isSecret ? 'env-var-secret' : 'env-var-active')
            : 'env-var-unknown',
          hoverMessage: hasKey
            ? { value: `**\`{{${key}}}\`** → \`${env!.variables[key] || '*empty (secret)*'}\`` }
            : { value: `**\`{{${key}}}\`** — unknown variable` },
        },
      });
    }

    if (!collection) {
      collection = editor.createDecorationsCollection(decorations);
    } else {
      collection.set(decorations);
    }
  };

  updateDecorations();
  editor.onDidChangeModelContent(updateDecorations);
}
