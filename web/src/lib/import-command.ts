export type ImportKind = 'curl' | 'grpcurl' | 'grpctestify';

export function importable(text: string): ImportKind | null {
  const line = text.trim().replace(/^\$\s+/, '');
  if (!line.includes(' ')) return null;
  const first = line.split(/\s+/)[0] ?? '';
  const name = (first.split('/').pop() ?? first).replace(/\.exe$/i, '').toLowerCase();
  if (name === 'curl') return 'curl';
  if (name === 'grpcurl') return 'grpcurl';
  if (name === 'grpctestify') return 'grpctestify';
  return null;
}
