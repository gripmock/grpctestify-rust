import { TLS_ALIASES, aliasValue, isTruthy } from './section-model';

type Kv = Record<string, string>;

const value = (tls: Kv, field: string) => aliasValue(tls, TLS_ALIASES[field]).trim();

export function caUnused(tls: Kv): boolean {
  return isTruthy(tls.insecure) && value(tls, 'ca_cert') !== '';
}

export function halfIdentity(tls: Kv): 'client_key' | 'client_cert' | null {
  const cert = value(tls, 'client_cert') !== '';
  const key = value(tls, 'client_key') !== '';
  if (cert === key) return null;
  return cert ? 'client_key' : 'client_cert';
}
