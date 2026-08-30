import { jsonStream } from './format';
import { attributeMeta, attributeTags } from './meta-tags';
import type { CollectionParsed, RequestTab } from './types';

export type Family = 'gctf' | 'httf';

export type SectionGroup = 'editor' | 'config' | 'view';

export type SectionDef = {
  key: RequestTab;
  label: string;
  group: SectionGroup;
  always: boolean;
  note?: string;
  count?: (p: CollectionParsed | null, bodies: string[], headers: Record<string, string>) => number;
};

const size = (o: Record<string, unknown> | undefined) => Object.keys(o ?? {}).length;
const named = (o: Record<string, unknown> | undefined) =>
  Object.keys(o ?? {}).filter(k => k.trim() !== '').length;

export const GCTF_SECTIONS: SectionDef[] = [
  { key: 'body', label: 'request', group: 'editor', always: true, count: (_p, bodies) => (bodies.length > 1 ? bodies.length : 0) },
  { key: 'headers', label: 'headers', group: 'editor', always: true, count: (_p, _b, headers) => named(headers) },
  {
    key: 'asserts',
    label: 'expect',
    group: 'editor',
    always: true,
    count: p =>
      (p?.asserts.length ?? 0)
      + (p?.expect_responses ?? []).reduce((n, m) => n + Math.max(1, jsonStream(m.body).messages), 0)
      + (p?.expect_error ? 1 : 0),
  },
  { key: 'extracts', label: 'extract', group: 'editor', always: true, count: p => size(p?.extracts) },
  { key: 'options', label: 'options', group: 'config', always: false, note: 'timeout, retries, compression and the transport this file uses', count: p => size(p?.options) },
  { key: 'tls', label: 'tls', group: 'config', always: false, note: 'how the connection is secured, and the client certificate it sends', count: p => size(p?.tls) },
  {
    key: 'meta',
    label: 'meta',
    group: 'config',
    always: false,
    note: 'name, owner and tags — what `run --tags` filters on and reports carry',
    count: p => {
      const attributes = p?.attributes ?? [];
      const tags = p?.meta_tags?.length ? p.meta_tags.length : attributeTags(attributes).length;
      return [
        p?.meta_name,
        p?.meta_owner || attributeMeta(attributes, 'owner'),
        p?.meta_summary || attributeMeta(attributes, 'summary'),
      ].filter(Boolean).length + tags + (p?.meta_links?.length ?? 0);
    },
  },
  { key: 'proto', label: 'proto', group: 'config', always: false, note: 'where the schema comes from: a descriptor, or .proto files', count: p => size(p?.proto) },
  { key: 'dataset', label: 'dataset', group: 'config', always: false, note: 'rows — the file becomes one case per row', count: p => (p?.dataset ?? []).length },
  { key: 'bench', label: 'bench', group: 'config', always: false, note: 'the load `bench` drives this file with', count: p => size(p?.bench) },
  { key: 'source', label: 'source', group: 'view', always: true },
  { key: 'plan', label: 'plan', group: 'view', always: true },
];

const GRPC_ONLY: RequestTab[] = ['proto', 'tls', 'bench'];

export const HTTF_SECTIONS: SectionDef[] = GCTF_SECTIONS.filter(d => !GRPC_ONLY.includes(d.key));

export function configKeys(family: Family): RequestTab[] {
  return defsFor(family).filter(d => d.group === 'config').map(d => d.key);
}

function defsFor(family: Family): SectionDef[] {
  return family === 'httf' ? HTTF_SECTIONS : GCTF_SECTIONS;
}

export function hiddenSections(
  p: CollectionParsed | null,
  bodies: string[],
  headers: Record<string, string>,
  family: Family = 'gctf',
): { key: RequestTab; label: string; note?: string }[] {
  return defsFor(family)
    .filter(d => !d.always && (d.count?.(p, bodies, headers) ?? 0) === 0)
    .map(d => (d.note === undefined ? { key: d.key, label: d.label } : { key: d.key, label: d.label, note: d.note }));
}

export function visibleSections(
  p: CollectionParsed | null,
  bodies: string[],
  headers: Record<string, string>,
  family: Family = 'gctf',
): { key: RequestTab; label: string; count?: number }[] {
  return defsFor(family)
    .filter(d => d.always || (d.count?.(p, bodies, headers) ?? 0) > 0)
    .map(d => {
      const n = d.count?.(p, bodies, headers) ?? 0;
      return n > 0 ? { key: d.key, label: d.label, count: n } : { key: d.key, label: d.label };
    });
}

export function sectionsByGroup(
  p: CollectionParsed | null,
  bodies: string[],
  headers: Record<string, string>,
  family: Family = 'gctf',
): Record<SectionGroup, { key: RequestTab; label: string; count?: number }[]> {
  const visible = visibleSections(p, bodies, headers, family);
  const groupOf = new Map(GCTF_SECTIONS.map(d => [d.key, d.group]));
  const out: Record<SectionGroup, { key: RequestTab; label: string; count?: number }[]> = {
    editor: [], config: [], view: [],
  };
  for (const s of visible) out[groupOf.get(s.key) ?? 'editor'].push(s);
  return out;
}
