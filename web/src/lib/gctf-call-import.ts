import { count } from 'luvo/data/plural';
import type { ImportedCommand } from './grpcurl-import';
import type { WireProtocol } from './types';

export interface ImportedCall extends ImportedCommand {
  file: string;
  docIndex: number | null;
  protocol: WireProtocol | '';
  insecure: boolean;
  ignored: string[];
}

const WITH_VALUE = new Set([
  '-e', '--endpoint', '-d', '--data', '--address', '--protocol', '-H', '--header',
  '--tls-ca', '--tls-cert', '--tls-key', '--doc-index', '--max-time', '--connect-timeout',
  '-o', '--output', '-D', '--dump-header', '-O', '--optimize', '--concurrency',
  '--requests', '--duration',
]);

const NOT_A_REQUEST = new Set([
  '-i', '--include', '-v', '--verbose', '--vv', '-s', '--silent', '-S', '--show-error',
  '-o', '--output', '-D', '--dump-header', '-O', '--optimize', '--bench', '--concurrency',
  '--requests', '--duration',
]);

const TLS_KEYS: Record<string, string> = {
  '--tls-ca': 'ca_cert',
  '--tls-cert': 'client_cert',
  '--tls-key': 'client_key',
};

export function isGrpctestify(command: string): boolean {
  const first = command.trim().replace(/^\$\s+/, '').split(/\s+/)[0] ?? '';
  const name = first.split('/').pop() ?? first;
  return name === 'grpctestify' || name === 'grpctestify.exe';
}

export function grpctestifySubcommand(args: string[]): string {
  return args.slice(1).find(a => !a.startsWith('-')) ?? '';
}

function splitHeader(raw: string): [string, string] | null {
  const at = raw.indexOf(':');
  if (at <= 0) return null;
  return [raw.slice(0, at).trim(), raw.slice(at + 1).trim()];
}

export function parseGrpctestifyCall(args: string[]): ImportedCall {
  const out: ImportedCall = {
    endpoint: '', address: '', headers: {}, body: '', plaintext: false,
    tls: {}, proto: {}, options: {},
    file: '', docIndex: null, protocol: '', insecure: false, ignored: [],
  };
  let rest = args.slice();
  if (rest[0] !== undefined && isGrpctestify(rest[0])) rest = rest.slice(1);
  if (rest[0] === 'call') rest = rest.slice(1);

  for (let i = 0; i < rest.length; i++) {
    const arg = rest[i]!;
    const takes = WITH_VALUE.has(arg);
    const value = takes ? rest[++i] ?? '' : '';
    if (NOT_A_REQUEST.has(arg)) {
      out.ignored.push(takes ? `${arg} ${value}` : arg);
      continue;
    }
    switch (arg) {
      case '-e': case '--endpoint': out.endpoint = value; break;
      case '-d': case '--data': out.body = value; break;
      case '--address': out.address = value; break;
      case '--protocol': {
        if (value === 'grpc' || value === 'grpc-web' || value === 'connectrpc') out.protocol = value;
        else out.ignored.push(`--protocol ${value}`);
        break;
      }
      case '--plaintext': out.plaintext = true; break;
      case '--insecure': out.insecure = true; break;
      case '--doc-index': {
        const step = Number(value);
        out.docIndex = Number.isFinite(step) && step > 0 ? Math.floor(step) : null;
        break;
      }
      case '--max-time': out.options = { ...out.options, 'max-time': value }; break;
      case '--connect-timeout': out.ignored.push(`${arg} ${value}`); break;
      case '-H': case '--header': {
        const pair = splitHeader(value);
        if (pair) out.headers[pair[0]] = pair[1];
        else out.ignored.push(`-H ${value}`);
        break;
      }
      default: {
        const tls = TLS_KEYS[arg];
        if (tls) { out.tls = { ...out.tls, [tls]: value }; break; }
        if (arg.startsWith('-')) { out.ignored.push(takes ? `${arg} ${value}` : arg); break; }
        if (out.file === '') out.file = arg;
        break;
      }
    }
  }
  out.ignored.sort();
  return out;
}

export function callSummary(imported: ImportedCall): string[] {
  const parts: string[] = [];
  const headers = Object.keys(imported.headers).length;
  if (headers > 0) parts.push(count(headers, 'header'));
  if (Object.keys(imported.tls ?? {}).length > 0) parts.push('TLS');
  if (imported.protocol !== '' && imported.protocol !== 'grpc') parts.push(imported.protocol);
  return parts;
}
