export type Row = Record<string, unknown>;

export function columnsOf(rows: unknown[]): string[] {
  const seen: string[] = [];
  for (const row of rows) {
    if (row === null || typeof row !== 'object' || Array.isArray(row)) continue;
    for (const key of Object.keys(row as Row)) {
      if (!seen.includes(key)) seen.push(key);
    }
  }
  return seen;
}

export function cellIn(value: string): unknown {
  const trimmed = value.trim();
  if (trimmed === '') return '';
  if (/^(true|false|null)$/.test(trimmed)) return JSON.parse(trimmed);
  if (/^-?\d+(\.\d+)?$/.test(trimmed)) return trimmed;
  if (/^[[{]/.test(trimmed)) {
    try { return JSON.parse(trimmed); } catch { return value; }
  }
  return value;
}

export function cellOut(value: unknown): string {
  if (value === undefined) return '';
  if (typeof value === 'string') return value;
  return JSON.stringify(value);
}

export function setCell(rows: unknown[], index: number, column: string, value: string): unknown[] {
  return rows.map((row, i) => {
    if (i !== index) return row;
    const next: Row = { ...(row as Row) };
    const parsed = cellIn(value);
    if (parsed === '') delete next[column];
    else next[column] = parsed;
    return next;
  });
}

export function addColumn(rows: unknown[], column: string): unknown[] {
  const name = column.trim();
  if (!name) return rows;
  const base = rows.length > 0 ? rows : [{}];
  return base.map(row => ({ ...(row as Row), [name]: (row as Row)[name] ?? '' }));
}

export function renameColumn(rows: unknown[], from: string, to: string): unknown[] {
  const name = to.trim();
  if (!name || name === from) return rows;
  return rows.map(row => {
    if (row === null || typeof row !== 'object' || Array.isArray(row)) return row;
    const out: Row = {};
    for (const [k, v] of Object.entries(row as Row)) {
      out[k === from ? name : k] = v;
    }
    return out;
  });
}

export function removeColumn(rows: unknown[], column: string): unknown[] {
  return rows.map(row => {
    if (row === null || typeof row !== 'object' || Array.isArray(row)) return row;
    const out: Row = { ...(row as Row) };
    delete out[column];
    return out;
  });
}

export function pruneRows(rows: unknown[]): unknown[] {
  return rows
    .map(row => {
      if (row === null || typeof row !== 'object' || Array.isArray(row)) return row;
      const out: Row = {};
      for (const [k, v] of Object.entries(row as Row)) {
        if (v !== '' && v !== undefined) out[k] = v;
      }
      return out;
    })
    .filter(row => row === null || typeof row !== 'object' || Array.isArray(row) || Object.keys(row as Row).length > 0);
}

export function addRow(rows: unknown[]): unknown[] {
  const columns = columnsOf(rows);
  const blank: Row = {};
  for (const c of columns) blank[c] = '';
  return [...rows, blank];
}

export function datasetUsage(columns: string[], texts: string[], inertTexts: string[] = []): {
  used: string[];
  unused: string[];
  missing: string[];
  inert: string[];
} {
  const names = (from: string[]) => {
    const found = new Set<string>();
    const PLACEHOLDER = /\{\{([^{}]*)\}\}/g;
    for (const text of from) {
      for (const match of (text ?? '').matchAll(PLACEHOLDER)) {
        const name = match[1].trim();
        if (name.startsWith('dataset.')) found.add(name.slice('dataset.'.length));
      }
    }
    return found;
  };
  const referenced = names(texts);
  const inert = names(inertTexts);
  return {
    used: columns.filter(c => referenced.has(c)),
    unused: columns.filter(c => !referenced.has(c) && !inert.has(c)),
    missing: [...referenced].filter(name => name !== '' && !columns.includes(name)),
    inert: [...inert].filter(name => name !== ''),
  };
}

export function renameDatasetRefs(text: string, from: string, to: string): string {
  const name = to.trim();
  if (!name || name === from || !text) return text;
  return text.replace(/\{\{([^{}]*)\}\}/g, (whole, body: string) => {
    const inner = body.trim();
    return inner === `dataset.${from}` ? `{{dataset.${name}}}` : whole;
  });
}

export function countDatasetRefs(texts: string[], column: string): number {
  let seen = 0;
  for (const text of texts) {
    for (const match of (text ?? '').matchAll(/\{\{([^{}]*)\}\}/g)) {
      if (match[1].trim() === `dataset.${column}`) seen += 1;
    }
  }
  return seen;
}
