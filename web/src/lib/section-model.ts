export type Kv = Record<string, string>;

export const COMPRESSION = ['none', 'gzip'] as const;
export const PROTOCOLS = ['grpc', 'grpc-web', 'connectrpc'] as const;
export const TLS_MODES = ['plaintext', 'tls', 'insecure'] as const;

export type TlsMode = (typeof TLS_MODES)[number];

export function setKey(kv: Kv, key: string, value: string): Kv {
  const next = { ...kv };
  if (value === '') delete next[key];
  else next[key] = value;
  return next;
}

export function numberValue(raw: string, opts: { min?: number; integer?: boolean } = {}): string | null {
  const trimmed = raw.trim();
  if (trimmed === '') return '';
  const n = Number(trimmed);
  if (!Number.isFinite(n)) return null;
  if (opts.integer && !Number.isInteger(n)) return null;
  if (opts.min !== undefined && n < opts.min) return null;
  return trimmed;
}

export function isTruthy(value: string | undefined): boolean {
  return value !== undefined && ['true', '1', 'yes', 'on'].includes(value.trim().toLowerCase());
}

export function tlsModeOf(tls: Kv): TlsMode {
  if (Object.keys(tls).length === 0) return 'plaintext';
  return isTruthy(tls.insecure) ? 'insecure' : 'tls';
}

export function applyTlsMode(tls: Kv, mode: TlsMode): Kv {
  if (mode === 'plaintext') return {};
  const next = { ...tls };
  if (mode === 'insecure') next.insecure = 'true';
  else delete next.insecure;
  return next;
}

export const TLS_ALIASES: Record<string, string[]> = {
  ca_cert: ['ca_cert', 'ca_file'],
  client_cert: ['client_cert', 'cert', 'cert_file'],
  client_key: ['client_key', 'key', 'key_file'],
  server_name: ['server_name'],
};

export function aliasValue(kv: Kv, aliases: string[]): string {
  for (const key of aliases) {
    const value = kv[key];
    if (value !== undefined) return value;
  }
  return '';
}

export function setAlias(kv: Kv, aliases: string[], value: string): Kv {
  const present = aliases.find(key => kv[key] !== undefined);
  return setKey(kv, present ?? aliases[0], value);
}

export function unknownKeys(kv: Kv, known: string[]): [string, string][] {
  const set = new Set(known);
  return Object.entries(kv).filter(([k]) => !set.has(k));
}

export const PROTO_SOURCES = ['reflection', 'descriptor', 'files'] as const;
export type ProtoSource = (typeof PROTO_SOURCES)[number];

export function protoSourceOf(proto: Kv): ProtoSource {
  if (proto.descriptor) return 'descriptor';
  if (proto.files) return 'files';
  return 'reflection';
}

export function applyProtoSource(proto: Kv, source: ProtoSource): Kv {
  if (source === 'reflection') return {};
  const next = { ...proto };
  if (source === 'descriptor') { delete next.files; delete next.import_paths; }
  else delete next.descriptor;
  return next;
}

export function csvList(value: string | undefined): string[] {
  return (value ?? '').split(',').map(s => s.trim()).filter(Boolean);
}

export function csvJoin(items: string[]): string {
  return items.filter(Boolean).join(',');
}
