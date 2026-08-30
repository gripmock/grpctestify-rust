import { looksLikeSecret } from './secret-names';

export type Origin = 'project' | 'browser';

export interface Row {
  key: string;
  value: string;
  local: boolean;
  shared?: string;
  placeholderInShared?: boolean;
}

export function shouldKeepLocal(row: Row, origin: Origin, next: string): boolean {
  return origin === 'project'
    && !row.local
    && !!row.placeholderInShared
    && next.trim() !== '';
}

export function hiddenValue(row: Row): boolean {
  return row.local || looksLikeSecret(row.key);
}

export const blankRow = (): Row => ({ key: '', value: '', local: false });

export function rowsOf(shared: [string, string][], local: [string, string][]): Row[] {
  const localMap = new Map(local);
  const rows: Row[] = shared.map(([key, value]) =>
    localMap.has(key)
      ? { key, value: localMap.get(key)!, local: true, shared: value }
      : { key, value, local: false, ...(value.trim() === '' ? { placeholderInShared: true } : {}) });
  const known = new Set(shared.map(([key]) => key));
  for (const [key, value] of local) if (!known.has(key)) rows.push({ key, value, local: true });
  return rows;
}

export function splitRows(rows: Row[]): { shared: [string, string][]; local: [string, string][] } {
  const named = rows.filter(r => r.key.trim());
  return {
    shared: named.map(r => [
      r.key.trim(),
      r.local ? (r.shared ?? '') : r.value,
    ] as [string, string]),
    local: named.filter(r => r.local).map(r => [r.key.trim(), r.value] as [string, string]),
  };
}

const ADDRESS_KEY = 'GRPC_ADDRESS';

export interface WithAddress {
  address: string;
  addressLocal: boolean;
  addressShared?: string;
  rows: Row[];
}

export function takeAddress(rows: Row[]): WithAddress {
  const row = rows.find(r => r.key.trim() === ADDRESS_KEY);
  return {
    address: row?.value ?? '',
    addressLocal: row?.local ?? false,
    ...(row?.shared !== undefined ? { addressShared: row.shared } : {}),
    rows: rows.filter(r => r.key.trim() !== ADDRESS_KEY),
  };
}

export function putAddress(rows: Row[], address: string, local: boolean, shared?: string): Row[] {
  const rest = rows.filter(r => r.key.trim() !== ADDRESS_KEY);
  if (!address.trim()) return rest;
  const row: Row = local
    ? { key: ADDRESS_KEY, value: address.trim(), local: true, shared: shared ?? '' }
    : { key: ADDRESS_KEY, value: address.trim(), local: false };
  return [row, ...rest];
}

export function missingNames(needed: string[], rows: Row[]): string[] {
  const have = new Set(rows.map(r => r.key.trim()).filter(Boolean));
  return needed.filter(n => n !== ADDRESS_KEY && !have.has(n));
}

export interface NameUse {
  name: string;
  count: number;
}

export function rankMissing(missing: string[], uses: NameUse[]): string[] {
  const count = new Map(uses.map(u => [u.name, u.count]));
  return [...missing].sort((a, b) => (count.get(b) ?? 0) - (count.get(a) ?? 0) || a.localeCompare(b));
}

export function filterNames(names: string[], query: string): string[] {
  const q = query.trim().toLowerCase();
  return q === '' ? names : names.filter(n => n.toLowerCase().includes(q));
}

export type RowState = 'blank' | 'set' | 'empty' | 'awaiting-local';

export function rowState(row: Row): RowState {
  if (row.key.trim() === '') return 'blank';
  if (row.value.trim() !== '') return 'set';
  return row.placeholderInShared ? 'awaiting-local' : 'empty';
}

export function duplicateNames(rows: Row[]): string[] {
  const seen = new Map<string, number>();
  for (const row of rows) {
    const key = row.key.trim();
    if (key === '') continue;
    seen.set(key, (seen.get(key) ?? 0) + 1);
  }
  return [...seen.entries()].filter(([, n]) => n > 1).map(([key]) => key);
}

export function overriddenRow(rows: Row[], at: number): boolean {
  const key = rows[at]?.key.trim();
  if (!key) return false;
  return rows.some((row, i) => i > at && row.key.trim() === key);
}

export function valueNamesVariable(value: string): string[] {
  const out: string[] = [];
  const pattern = /\{\{\s*([A-Za-z_][A-Za-z0-9_.]*)\s*\}\}/g;
  let found = pattern.exec(value);
  while (found !== null) {
    if (!out.includes(found[1]!)) out.push(found[1]!);
    found = pattern.exec(value);
  }
  return out;
}
