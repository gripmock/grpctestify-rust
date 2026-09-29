export type DatasetRow = Record<string, unknown>;

export function rowsOf(dataset: unknown[] | undefined | null): DatasetRow[] {
  return (dataset ?? []).filter(
    (row): row is DatasetRow => row !== null && typeof row === 'object' && !Array.isArray(row),
  );
}

export function rowValues(dataset: unknown[] | undefined | null, index: number): Record<string, string> | null {
  const row = rowsOf(dataset)[index];
  if (!row) return null;
  return Object.fromEntries(
    Object.entries(row).map(([k, v]) => [k, typeof v === 'string' ? v : JSON.stringify(v)]),
  );
}

export function rowLabel(dataset: unknown[] | undefined | null, index: number): string | null {
  const rows = rowsOf(dataset);
  const row = rows[index];
  if (!row) return null;
  const bound = Object.entries(row)
    .map(([k, v]) => `${k}=${typeof v === 'string' ? v : JSON.stringify(v)}`)
    .join(' ');
  return `row ${index + 1} of ${rows.length}${bound === '' ? '' : ` · ${bound}`}`;
}

export function clampRow(dataset: unknown[] | undefined | null, index: number): number {
  const total = rowsOf(dataset).length;
  if (total === 0) return 0;
  return Math.min(Math.max(0, index), total - 1);
}
