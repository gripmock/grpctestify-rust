import { MASK, looksLikeSecret } from './secret-names';

const SECRET_KEYS = [
  'authorization',
  'proxy-authorization',
  'cookie',
  'set-cookie',
  'x-api-key',
  'api-key',
];

export function isSecretHeader(key: string): boolean {
  const lower = key.trim().toLowerCase();
  return SECRET_KEYS.includes(lower) || lower.endsWith('-token');
}

export function hidesTyped(key: string, value: string): boolean {
  return isSecretHeader(key) && !value.includes('{{');
}

export function maskHeader(key: string, value: string): string {
  if (value === '' || value.includes('{{')) return value;
  return isSecretHeader(key) || looksLikeSecret(key) ? MASK : value;
}

export function splitScheme(value: string): { prefix: string; secret: string } {
  const match = /^(Bearer|Basic|Token|Digest|ApiKey)\s+/i.exec(value.trim());
  if (!match) return { prefix: '', secret: value.trim() };
  return { prefix: match[0].replace(/\s+$/, ' '), secret: value.trim().slice(match[0].length) };
}

export function variableNameFor(key: string): string {
  const lower = key.trim().toLowerCase();
  if (lower === 'authorization' || lower === 'proxy-authorization') return 'AUTH_TOKEN';
  if (lower === 'cookie' || lower === 'set-cookie') return 'COOKIE';
  if (lower === 'x-api-key' || lower === 'api-key') return 'API_KEY';
  return lower.replace(/^x-/, '').replace(/[^a-z0-9]+/g, '_').replace(/^_+|_+$/g, '').toUpperCase()
    || 'TOKEN';
}
