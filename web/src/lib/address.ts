import { defaultAddressFor, type WireProtocol } from './types';
import type { Family } from './sections';
import { looksHttp } from './http-endpoint';

export interface AddressCheck {
  ok: boolean;
  reason?: string;
  note?: string;
}

const OK: AddressCheck = { ok: true };
const HOST = /^[A-Za-z0-9._-]+$/;

export function checkAddress(raw: string, family: Family = 'gctf'): AddressCheck {
  const value = raw.trim();
  if (value === '') return OK;
  if (value.includes('{{')) return OK;
  if (/\s/.test(value)) return { ok: false, reason: 'An address has no spaces' };

  const authority = value.includes('://') ? value.slice(value.indexOf('://') + 3) : value;
  if (authority === '') return { ok: false, reason: 'No host after the scheme' };

  const path = authority.indexOf('/');
  const hostPort = path === -1 ? authority : authority.slice(0, path);
  const dropped = family === 'gctf' && path !== -1 && authority.slice(path + 1) !== ''
    ? `A gRPC address is a host and a port: the path in "${value}" is not dialled`
    : undefined;
  const withNote = (check: AddressCheck): AddressCheck =>
    (check.ok && dropped ? { ...check, note: dropped } : check);

  if (hostPort.startsWith('[')) {
    const close = hostPort.indexOf(']');
    if (close === -1) return { ok: false, reason: 'An IPv6 address needs its closing bracket' };
    return withNote(checkPort(hostPort.slice(close + 1), value.includes('://')));
  }

  const colon = hostPort.lastIndexOf(':');
  const host = colon === -1 ? hostPort : hostPort.slice(0, colon);
  if (host === '') return { ok: false, reason: 'No host' };
  if (host.includes(':')) return { ok: false, reason: 'An IPv6 address goes in brackets — [::1]:50051' };
  if (!HOST.test(host)) return { ok: false, reason: `"${host}" is not a host name` };

  const portOptional = family === 'httf' || value.includes('://');
  return withNote(checkPort(colon === -1 ? '' : hostPort.slice(colon), portOptional));
}

function checkPort(rest: string, portOptional: boolean): AddressCheck {
  if (rest === '') {
    return portOptional ? OK : { ok: false, reason: 'No port — grpctestify dials host:port' };
  }
  const port = rest.slice(1);
  if (!/^[0-9]+$/.test(port)) return { ok: false, reason: `"${port}" is not a port` };
  const n = Number(port);
  if (n < 1 || n > 65535) return { ok: false, reason: 'A port is between 1 and 65535' };
  return OK;
}

export interface EffectiveAddress {
  address: string;
  source: 'file' | 'client';
  overridden: boolean;
}

export function effectiveAddress(typed: string, fileAddress: string | null | undefined): EffectiveAddress {
  const file = (fileAddress ?? '').trim();
  const client = typed.trim();
  if (file === '') return { address: client, source: 'client', overridden: false };
  return { address: file, source: 'file', overridden: client !== '' && file !== client };
}

export type AddressSource = 'file' | 'typed' | 'environment' | 'server' | 'default';

export interface AddressDecision {
  address: string;
  source: AddressSource;
  why: string;
}

export function addressDecision(input: {
  file?: string | null;
  fileFromChain?: boolean;
  typed: string;
  environment?: string | null;
  server?: string | null;
  fallback: string;
}): AddressDecision {
  const file = (input.file ?? '').trim();
  if (file !== '') {
    return {
      address: file,
      source: 'file',
      why: input.fileFromChain
        ? 'the address the chain started with'
        : 'the ADDRESS section of this file',
    };
  }

  const typed = input.typed.trim();
  if (typed !== '') return { address: typed, source: 'typed', why: 'the address in the header' };

  const environment = (input.environment ?? '').trim();
  if (environment !== '') {
    return { address: environment, source: 'environment', why: 'the address of the active environment' };
  }

  const server = (input.server ?? '').trim();
  if (server !== '') {
    return { address: server, source: 'server', why: 'GRPCTESTIFY_ADDRESS, how this server was started' };
  }

  if (input.fallback.trim() === '') {
    return { address: '', source: 'default', why: 'nothing names a target yet — an HTTP call needs an address' };
  }

  return { address: input.fallback, source: 'default', why: "the transport's default" };
}

export function runDivergence(
  execute: AddressDecision,
  run: AddressDecision,
  hasFile: boolean,
): AddressDecision | null {
  if (!hasFile) return null;
  return execute.address === run.address ? null : run;
}

const HTTP_ADDRESS_HINT = 'https://api.example.com';

export function addressPlaceholder(input: {
  file?: string | null;
  environment?: string | null;
  server?: string | null;
  protocol: WireProtocol;
  family?: Family;
}): string {
  const known = input.file || input.environment || input.server;
  if (known) return known;
  return input.family === 'httf' ? HTTP_ADDRESS_HINT : defaultAddressFor(input.protocol);
}

export function chainAddressAt(
  steps: { address: string; address_source: 'section' | 'inherited'; endpoint?: string }[],
  index: number,
): string {
  return chainAddressSource(steps, index).address;
}

export function chainAddressSource(
  steps: { address: string; address_source: 'section' | 'inherited'; endpoint?: string }[],
  index: number,
): { address: string; from: number } {
  const at = Math.min(index, steps.length - 1);
  const mine = steps[at] ? looksHttp(steps[at].endpoint ?? '') : false;
  for (let i = at; i >= 0; i--) {
    const step = steps[i];
    if (step.endpoint !== undefined && looksHttp(step.endpoint) !== mine) continue;
    if (step.address_source === 'section' && step.address.trim() !== '') {
      return { address: step.address.trim(), from: i };
    }
  }
  return { address: '', from: -1 };
}
