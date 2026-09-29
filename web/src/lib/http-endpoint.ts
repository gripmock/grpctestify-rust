import { importable } from './import-command';
export const METHODS = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS'] as const;

export interface HttpEndpoint {
  method: string;
  path: string;
}

export function splitEndpoint(endpoint: string): HttpEndpoint {
  const trimmed = endpoint.trim();
  const at = trimmed.indexOf(' ');
  if (at < 0) return { method: trimmed.toUpperCase(), path: '' };
  return { method: trimmed.slice(0, at).toUpperCase(), path: trimmed.slice(at + 1).trim() };
}

export function joinEndpoint(method: string, path: string): string {
  const verb = method.trim().toUpperCase();
  const rest = path.trim();
  if (!verb) return rest;
  return rest ? `${verb} ${rest}` : verb;
}

export function pathIssue(path: string): string | null {
  const value = path.trim();
  if (!value) return 'a path is missing';
  if (/^https?:\/\//.test(value)) return null;
  if (value.startsWith('{{')) return null;
  if (!value.startsWith('/')) return 'a relative path starts with /';
  return null;
}

export function looksHttp(endpoint: string): boolean {
  if (importable(endpoint)) return false;
  const { method, path } = splitEndpoint(endpoint);
  return method !== '' && path !== '' && /^[A-Z-]{3,24}$/.test(method) && !method.includes('/');
}

export function isHttpRequest(path: string | null | undefined, endpoint: string): boolean {
  const name = path ?? '';
  if (name.endsWith('.httf')) return true;
  if (name.endsWith('.gctf')) return false;
  return looksHttp(endpoint);
}

export function shortTarget(address: string | null | undefined): string {
  const target = (address ?? '').trim();
  const bare = target.replace(/^http:\/\//i, '');
  return bare === '' ? target : bare;
}

export function httpUrl(address: string, path: string): string {
  const target = path.trim();
  if (/^https?:\/\//.test(target)) return target;
  const base = address.trim().replace(/\/$/, '');
  if (!base) return target;
  const origin = /^https?:\/\//.test(base) ? base : `http://${base}`;
  if (!target) return origin;
  return target.startsWith('/') ? `${origin}${target}` : `${origin}/${target}`;
}

export function draftFileName(path: string | null | undefined, endpoint: string): string {
  if ((path ?? '').endsWith('.apif')) return 'playground.apif';
  return isHttpRequest(path, endpoint) ? 'playground.httf' : 'playground.gctf';
}

export function noHostYet(isHttp: boolean, address: string, path: string): boolean {
  if (!isHttp || address.trim() !== '') return false;
  return !/^https?:\/\//.test(path.trim());
}

export function requestFamily(path: string | null | undefined, endpoint: string): 'gctf' | 'httf' {
  return isHttpRequest(path, endpoint) ? 'httf' : 'gctf';
}

export type MethodTone = 'read' | 'write' | 'destructive' | 'other';

export function methodTone(method: string): MethodTone {
  switch (method.trim().toUpperCase()) {
    case 'GET':
    case 'HEAD':
    case 'OPTIONS':
    case 'TRACE':
      return 'read';
    case 'POST':
    case 'PUT':
    case 'PATCH':
      return 'write';
    case 'DELETE':
      return 'destructive';
    default:
      return 'other';
  }
}

export function suggestedFileName(endpoint: string): string {
  const trimmed = endpoint.trim();
  if (trimmed === '') return '';
  if (!looksHttp(trimmed)) return trimmed.split('/').pop() ?? '';
  const { method, path } = splitEndpoint(trimmed);
  const bare = path.split('?')[0].split('#')[0];
  const segment = bare
    .split('/')
    .map(part => part.replace(/\{\{\s*|\s*\}\}/g, '').trim())
    .filter(part => part !== '')
    .pop();
  return segment || method.toLowerCase();
}
