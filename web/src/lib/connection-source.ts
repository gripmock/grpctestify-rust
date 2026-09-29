import { tlsModeOf, type TlsMode } from './section-model';
import { timeoutSeconds } from './format';
import type { CollectionParsed, WireProtocol } from './types';

export interface FromFile<T> {
  value: T;
  differs: boolean;
}

export interface ConnectionFromFile {
  protocol: FromFile<WireProtocol> | null;
  tls: FromFile<TlsMode> | null;
}

const PROTOCOLS: WireProtocol[] = ['grpc', 'grpc-web', 'connectrpc'];

export function connectionFromFile(
  parsed: CollectionParsed | null,
  client: { protocol: WireProtocol; tls: boolean; tlsInsecure: boolean },
): ConnectionFromFile {
  const named = parsed?.options?.protocol;
  const protocol = PROTOCOLS.includes(named as WireProtocol)
    ? { value: named as WireProtocol, differs: named !== client.protocol }
    : null;

  const section = parsed?.tls ?? {};
  const tls = Object.keys(section).length > 0
    ? (() => {
        const mode = tlsModeOf(section);
        const here: TlsMode = !client.tls ? 'plaintext' : client.tlsInsecure ? 'insecure' : 'tls';
        return { value: mode, differs: mode !== here };
      })()
    : null;

  return { protocol, tls };
}

export function fileConnectionNote(from: ConnectionFromFile): string {
  const parts: string[] = [];
  if (from.protocol) parts.push(`OPTIONS.protocol: ${from.protocol.value}`);
  if (from.tls) parts.push(`TLS: ${from.tls.value}`);
  return parts.join(' · ');
}

export function connectionUsed(
  parsed: CollectionParsed | null,
  client: { protocol: WireProtocol; tls: boolean; tlsInsecure: boolean },
): { protocol: WireProtocol; tls: boolean; tlsInsecure: boolean } {
  const from = connectionFromFile(parsed, client);
  const mode = from.tls?.value;
  return {
    protocol: from.protocol?.value ?? client.protocol,
    tls: mode === undefined ? client.tls : mode !== 'plaintext',
    tlsInsecure: mode === undefined ? client.tlsInsecure : mode === 'insecure',
  };
}

export interface TimeoutUsed {
  seconds: number;
  source: 'file' | 'workbench' | 'default';
  from?: 'attribute' | 'options';
}

export function timeoutUsed(parsed: CollectionParsed | null, clientMs: number): TimeoutUsed {
  const attribute = attributeOf(parsed, 'timeout');
  const named = Number(attribute ?? parsed?.options?.timeout);
  if (Number.isFinite(named) && named > 0) {
    return {
      seconds: Math.floor(named),
      source: 'file',
      from: attribute === undefined ? 'options' : 'attribute',
    };
  }
  const here = timeoutSeconds(clientMs);
  if (here > 0) return { seconds: here, source: 'workbench' };
  return { seconds: 30, source: 'default' };
}

function attributeOf(parsed: CollectionParsed | null, name: string): string | undefined {
  return parsed?.attributes?.find(a => a.name === name)?.value;
}

export function compressionFromFile(parsed: CollectionParsed | null): 'gzip' | null {
  const named = String(attributeOf(parsed, 'compression') ?? parsed?.options?.compression ?? '')
    .trim()
    .toLowerCase();
  return named === 'gzip' ? 'gzip' : null;
}
