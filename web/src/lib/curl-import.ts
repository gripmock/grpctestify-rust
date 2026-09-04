import { count } from 'luvo/data/plural';
export interface ImportedCurl {
  method: string;
  url: string;
  address: string;
  path: string;
  headers: Record<string, string>;
  body: string;
  insecure: boolean;
  ignored: string[];
}

const WITH_VALUE = new Set([
  '-H', '--header', '-X', '--request', '-d', '--data', '--data-raw', '--data-binary',
  '--data-urlencode', '--json', '--url', '-u', '--user', '-A', '--user-agent', '-b',
  '--cookie', '-e', '--referer', '-m', '--max-time', '--connect-timeout', '-F', '--form',
  '-o', '--output', '-T', '--upload-file', '-w', '--write-out', '-D', '--dump-header',
  '-c', '--cookie-jar', '-E', '--cert', '--key', '--cacert', '--capath', '--key-type',
  '--cert-type', '-x', '--proxy', '--proxy-user', '--resolve', '--retry', '--retry-delay',
  '--retry-max-time', '--limit-rate', '--interface', '--oauth2-bearer', '-K', '--config',
  '--max-filesize', '--expect100-timeout', '--proto', '--proto-default', '--range', '-r',
]);

const CARRIES_SECRET = new Set([
  '-u', '--user', '-b', '--cookie', '--oauth2-bearer', '--proxy-user', '--key', '-E', '--cert',
]);

const IGNORED_FLAGS = new Set([
  '--compressed', '-s', '--silent', '-S', '--show-error', '-L', '--location', '-i',
  '--include', '-v', '--verbose', '-g', '--globoff', '--no-buffer',
]);

function splitHeader(raw: string): [string, string] | null {
  const at = raw.indexOf(':');
  if (at <= 0) return null;
  return [raw.slice(0, at).trim(), raw.slice(at + 1).trim()];
}

export function isCurl(command: string): boolean {
  const first = command.trim().replace(/^\$\s+/, '').split(/\s+/)[0] ?? '';
  const name = first.split('/').pop() ?? first;
  return name === 'curl' || name === 'curl.exe';
}

export function parseCurl(args: string[]): ImportedCurl {
  const headers: Record<string, string> = {};
  const ignored: string[] = [];
  let method = '';
  let url = '';
  let body = '';
  let insecure = false;
  let form = false;
  let asQuery = false;

  const named = args[0] !== undefined && isCurl(args[0]);
  for (let i = named ? 1 : 0; i < args.length; i++) {
    const arg = args[i];
    const take = () => args[++i] ?? '';

    if (arg === '-k' || arg === '--insecure') { insecure = true; continue; }
    if (arg === '-G' || arg === '--get') { asQuery = true; continue; }
    if (arg === '-I' || arg === '--head') { if (!method) method = 'HEAD'; continue; }
    if (IGNORED_FLAGS.has(arg)) continue;

    if (arg === '-X' || arg === '--request') { method = take().toUpperCase(); continue; }
    if (arg === '--url') { url = take(); continue; }
    if (arg === '-H' || arg === '--header') {
      const pair = splitHeader(take());
      if (pair) headers[pair[0]] = pair[1];
      continue;
    }
    if (arg === '-d' || arg === '--data' || arg === '--data-raw' || arg === '--data-binary') {
      body = body ? `${body}&${take()}` : take();
      continue;
    }
    if (arg === '--data-urlencode') {
      const raw = take();
      if (raw.includes('@')) {
        ignored.push(`${arg} ${raw} (the file itself is not imported)`);
        continue;
      }
      const piece = urlEncoded(raw);
      body = body ? `${body}&${piece}` : piece;
      continue;
    }
    if (arg === '--json') {
      body = take();
      if (!headers['content-type'] && !headers['Content-Type']) headers['content-type'] = 'application/json';
      continue;
    }
    if (arg === '-F' || arg === '--form') {
      take();
      form = true;
      continue;
    }
    if (arg === '-T' || arg === '--upload-file') {
      const file = take();
      if (!method) method = 'PUT';
      ignored.push(`${arg} ${file} (the file itself is not imported)`);
      continue;
    }
    if (arg.startsWith('-')) {
      const value = WITH_VALUE.has(arg) ? take() : '';
      ignored.push(value && !CARRIES_SECRET.has(arg) ? `${arg} ${value}` : arg);
      continue;
    }
    if (!url) url = arg;
  }

  if (form) ignored.push('-F (a multipart body is not imported)');
  if (asQuery && body) {
    url = `${url}${url.includes('?') ? '&' : '?'}${body}`;
    body = '';
  }
  if (!method) method = body ? 'POST' : 'GET';

  const { address, path } = splitUrl(url);
  return { method, url, address, path, headers, body, insecure, ignored: ignored.sort() };
}

function urlEncoded(raw: string): string {
  const at = raw.indexOf('=');
  if (at === -1) return encodeURIComponent(raw);
  const name = raw.slice(0, at);
  const value = encodeURIComponent(raw.slice(at + 1));
  return name === '' ? value : `${name}=${value}`;
}

export function splitUrl(url: string): { address: string; path: string } {
  const trimmed = url.trim();
  const match = /^(https?:\/\/[^/?#]+)(.*)$/.exec(trimmed);
  if (!match) return { address: '', path: trimmed };
  return { address: match[1], path: match[2] === '' ? '/' : match[2] };
}

export function curlSummary(imported: ImportedCurl): string[] {
  const lines = [`${imported.method} ${imported.path || '/'}`];
  if (imported.address) lines.push(`address ${imported.address}`);
  const headerCount = Object.keys(imported.headers).length;
  if (headerCount > 0) lines.push(`${count(headerCount, 'header')}`);
  if (imported.body) lines.push(`a body of ${imported.body.length} characters`);
  if (imported.insecure) lines.push('certificate checks off — not carried, https is https here');
  return lines;
}

function shellQuote(value: string): string {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

export interface CurlOut {
  method: string;
  url: string;
  headers: Record<string, string>;
  body: string;
}

export function toCurl({ method, url, headers, body }: CurlOut): string {
  const parts = ['curl', '-L'];
  if (method && method !== 'GET') parts.push('-X', method);
  parts.push(shellQuote(url));
  for (const [name, value] of Object.entries(headers)) {
    if (!name.trim()) continue;
    parts.push('-H', shellQuote(`${name}: ${value}`));
  }
  if (body.trim()) parts.push('-d', shellQuote(body));
  return parts.join(' ');
}
