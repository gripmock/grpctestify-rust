export type DataChoice = { path: string; columns: string[] };

export function rememberedChoice(saved: unknown): DataChoice | null {
  if (!saved || typeof saved !== 'object') return null;
  const path = (saved as { path?: unknown }).path;
  if (typeof path !== 'string' || path === '') return null;
  const columns = (saved as { columns?: unknown }).columns;
  return {
    path,
    columns: Array.isArray(columns) ? columns.filter((c): c is string => typeof c === 'string') : [],
  };
}

export function reconcileChoice(
  path: string | null,
  sources: { path: string; columns?: string[] }[],
): DataChoice | null {
  if (path === null) return null;
  const source = sources.find(s => s.path === path);
  if (!source) return null;
  return { path, columns: source.columns ?? [] };
}
