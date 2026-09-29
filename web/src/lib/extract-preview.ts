export type ExtractValue =
  | { kind: 'value'; text: string }
  | { kind: 'null' }
  | { kind: 'none' }
  | { kind: 'many'; count: number; text: string }
  | { kind: 'error'; reason: string };

export function extractValue(outputs: unknown[] | undefined, error: string | null | undefined): ExtractValue {
  if (error) return { kind: 'error', reason: error };
  const values = outputs ?? [];
  if (values.length === 0) return { kind: 'none' };
  if (values.length === 1 && values[0] === null) return { kind: 'null' };
  const text = format(values[0]);
  if (values.length > 1) return { kind: 'many', count: values.length, text };
  return { kind: 'value', text };
}

function format(value: unknown): string {
  if (typeof value === 'string') return value;
  return JSON.stringify(value) ?? String(value);
}

export function extractLabel(value: ExtractValue): string {
  switch (value.kind) {
    case 'value': return value.text;
    case 'many': return `${value.text} — and ${value.count - 1} more`;
    case 'null': return 'matched null — the next step sends null';
    case 'none': return 'nothing matched — the run fails here';
    case 'error': return value.reason;
  }
}
