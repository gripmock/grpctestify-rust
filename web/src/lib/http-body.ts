import { jsonProblem } from './format';

export function contentTypeOf(body: string): string {
  if (jsonProblem(body) === null && body.trim() !== '') return 'application/json';
  const trimmed = body.replace(/^\s+/, '');
  if (trimmed.startsWith('<?xml') || trimmed.startsWith('<')) return 'application/xml';
  const oneLine = trimmed.split('\n').length === 1;
  if (trimmed !== '' && oneLine && trimmed.includes('=') && !trimmed.includes(' ')) {
    return 'application/x-www-form-urlencoded';
  }
  return 'text/plain';
}

export function declaredContentType(headers: Record<string, string>): string | null {
  const found = Object.entries(headers).find(([k]) => k.trim().toLowerCase() === 'content-type');
  return found && found[1].trim() !== '' ? found[1] : null;
}

export type PreviewKind = 'html' | 'svg';

export function previewKind(headers: Record<string, string>, body: unknown): PreviewKind | null {
  if (typeof body !== 'string' || body.trim() === '') return null;
  const declared = (declaredContentType(headers) ?? '').split(';')[0].trim().toLowerCase();
  if (declared === 'text/html' || declared === 'application/xhtml+xml') return 'html';
  if (declared === 'image/svg+xml') return 'svg';
  return null;
}

const BINARY_EXACT = new Set([
  'application/octet-stream', 'application/pdf', 'application/zip', 'application/gzip',
  'application/x-gzip', 'application/x-tar', 'application/wasm', 'application/x-protobuf',
  'application/protobuf', 'application/grpc',
]);

export function binaryType(headers: Record<string, string>): string | null {
  const declared = (declaredContentType(headers) ?? '').split(';')[0].trim().toLowerCase();
  if (declared === '') return null;
  if (declared === 'image/svg+xml') return null;
  const family = declared.split('/')[0];
  if (family === 'image' || family === 'audio' || family === 'video' || family === 'font') return declared;
  return BINARY_EXACT.has(declared) ? declared : null;
}

export function wireBytes(headers: Record<string, string>): number | null {
  const found = Object.entries(headers).find(([k]) => k.trim().toLowerCase() === 'content-length');
  if (!found) return null;
  const value = Number(found[1].trim());
  return Number.isFinite(value) && value >= 0 ? value : null;
}

export function bodyWithoutAMethodForIt(method: string, bodies: string[]): boolean {
  const verb = method.trim().toUpperCase();
  if (verb !== 'GET' && verb !== 'HEAD') return false;
  return bodies.some(body => (body ?? '').trim() !== '');
}
