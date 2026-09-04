export type DotenvLine =
  | { kind: 'entry'; key: string; value: string; export: boolean }
  | { kind: 'other'; raw: string };

const ENTRY = /^(\s*)(?:export\s+)?([A-Za-z_][A-Za-z0-9_.]*)\s*=(.*)$/;

function unquote(raw: string): string {
  const value = raw.trim();
  if (value.length >= 2) {
    const first = value[0];
    const last = value[value.length - 1];
    if ((first === '"' || first === "'") && last === first) {
      const inner = value.slice(1, -1);
      return first === '"' ? inner.replace(/\\n/g, '\n').replace(/\\"/g, '"') : inner;
    }
  }
  return value.replace(/\s+#.*$/, '').trim();
}

function quote(value: string): string {
  if (value === '') return '';
  if (/^[^\s#'"][^\s#]*$/.test(value)) return value;
  return `"${value.replace(/\\/g, '\\\\').replace(/"/g, '\\"').replace(/\n/g, '\\n')}"`;
}

export function parseDotenv(text: string): DotenvLine[] {
  if (!text) return [];
  return text.replace(/\r\n/g, '\n').split('\n').map(raw => {
    const match = ENTRY.exec(raw);
    if (!match || raw.trimStart().startsWith('#')) return { kind: 'other', raw } as DotenvLine;
    return {
      kind: 'entry',
      key: match[2],
      value: unquote(match[3]),
      export: /^\s*export\s/.test(raw),
    } as DotenvLine;
  });
}

export function serializeDotenv(lines: DotenvLine[]): string {
  const text = lines
    .map(line => (line.kind === 'other' ? line.raw : `${line.export ? 'export ' : ''}${line.key}=${quote(line.value)}`))
    .join('\n');
  return text.endsWith('\n') || text === '' ? text : `${text}\n`;
}

export function entriesOf(lines: DotenvLine[]): [string, string][] {
  const out = new Map<string, string>();
  for (const line of lines) if (line.kind === 'entry') out.set(line.key, line.value);
  return [...out];
}

export function applyEntries(lines: DotenvLine[], entries: [string, string][]): DotenvLine[] {
  const wanted = new Map(entries.filter(([k]) => k.trim()));
  const seen = new Set<string>();

  const kept = lines.filter(line => {
    if (line.kind !== 'entry') return true;
    return wanted.has(line.key);
  }).map(line => {
    if (line.kind !== 'entry') return line;
    seen.add(line.key);
    return { ...line, value: wanted.get(line.key)! };
  });

  const added: DotenvLine[] = [...wanted]
    .filter(([key]) => !seen.has(key))
    .map(([key, value]) => ({ kind: 'entry', key, value, export: false }));

  if (added.length === 0) return kept;
  const tail = kept.length > 0 && kept[kept.length - 1].kind === 'other'
    && (kept[kept.length - 1] as { raw: string }).raw.trim() === '';
  return tail ? [...kept.slice(0, -1), ...added, kept[kept.length - 1]] : [...kept, ...added];
}
