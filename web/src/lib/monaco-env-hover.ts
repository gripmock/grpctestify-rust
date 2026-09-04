import { isVariableName } from './env';
import { maskValue } from './secret-names';
import { resolvedElsewhere, type RuntimeNames } from './env-usage';

export function variableAt(line: string, column: number): { key: string; start: number; end: number } | null {
  const regex = /\{\{([^{}]*)\}\}/g;
  let match: RegExpExecArray | null;
  while ((match = regex.exec(line)) !== null) {
    const start = match.index + 1;
    const end = start + match[0].length;
    if (column >= start && column < end) {
      const key = match[1].trim();
      return isVariableName(key) ? { key, start, end } : null;
    }
  }
  return null;
}

export function registerEnvHoverProvider(
  monaco: any,
  getEnv: () => {
    name: string;
    variables: Record<string, string>;
    mutedVariables?: string[];
    secret?: string[];
    source?: 'project' | 'browser';
  } | null | undefined,
) {
  const langs = monaco.languages.getLanguages?.()?.map((l: any) => l.id) || ['json', 'plaintext'];

  for (const lang of langs) {
    monaco.languages.registerHoverProvider(lang, {
      provideHover(model: any, position: any) {
        const env = getEnv();
        if (!env?.variables) return null;

        const found = variableAt(model.getLineContent(position.lineNumber), position.column);
        if (!found) return null;

        const key = found.key;
        const val = env.variables[key];
        if (val === undefined) return null;

        const range = {
          startLineNumber: position.lineNumber,
          startColumn: found.start,
          endLineNumber: position.lineNumber,
          endColumn: found.end,
        };

        const muted = (env.mutedVariables || []).includes(key);
        const shown = maskValue(key, val, env.secret);
        const lines = [
          `**\`{{${key}}}\`** → ` + (val ? `\`${shown}\`` : '*empty (secret)*'),
          env.source === 'browser'
            ? `from **${env.name}** — this browser's own`
            : `from **${env.name}** — the project's \`.env.${env.name}\``,
        ];
        if (muted) lines.push('_muted — excluded from substitution_');

        return { range, contents: [{ value: lines.join('  \n') }] };
      },
    });
  }
}

export function addEnvDecorations(
  editor: any,
  monaco: any,
  getEnv: () => { variables: Record<string, string>; mutedVariables?: string[] } | null | undefined,
  getRuntime: () => RuntimeNames = () => ({}),
) {
  let collection: any = null;

  const updateDecorations = () => {
    const model = editor.getModel();
    if (!model) return;

    const text = model.getValue();
    const regex = /\{\{([^{}]*)\}\}/g;
    const decorations: any[] = [];
    let match: RegExpExecArray | null;

    while ((match = regex.exec(text)) !== null) {
      const key = match[1].trim();
      if (!isVariableName(key)) continue;
      const env = getEnv();
      const hasKey = env?.variables?.[key] !== undefined;
      const muted = hasKey && (env?.mutedVariables || []).includes(key);
      const isSecret = hasKey && !env?.variables[key];
      const elsewhere = hasKey ? null : resolvedElsewhere(key, getRuntime());

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
            : elsewhere ? 'env-var-runtime' : 'env-var-unknown',
          hoverMessage: hasKey
            ? undefined
            : elsewhere === 'dataset'
              ? { value: `**\`{{${key}}}\`** — a DATASET column` }
              : elsewhere === 'extract'
                ? { value: `**\`{{${key}}}\`** — extracted by an earlier step` }
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
  return updateDecorations;
}
