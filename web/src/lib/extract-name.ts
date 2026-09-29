export interface ExtractName {
  name: string;
  kind: string | null;
}

export function splitExtractName(written: string): ExtractName {
  const trimmed = written.trim();
  const at = trimmed.lastIndexOf(':');
  if (at === -1) return { name: trimmed, kind: null };
  const kind = trimmed.slice(at + 1).trim();
  if (kind === '' || !/^[A-Za-z0-9_]+$/.test(kind)) return { name: trimmed, kind: null };
  return { name: trimmed.slice(0, at).trim(), kind };
}

export function writtenExtractName(name: string, kind: string | null | undefined): string {
  return kind ? `${name}:${kind}` : name;
}
