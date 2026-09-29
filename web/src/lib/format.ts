const GRPC_STATUS: Record<number, string> = {
  0: 'OK', 1: 'CANCELLED', 2: 'UNKNOWN', 3: 'INVALID_ARGUMENT', 4: 'DEADLINE_EXCEEDED',
  5: 'NOT_FOUND', 6: 'ALREADY_EXISTS', 7: 'PERMISSION_DENIED', 8: 'RESOURCE_EXHAUSTED',
  9: 'FAILED_PRECONDITION', 10: 'ABORTED', 11: 'OUT_OF_RANGE', 12: 'UNIMPLEMENTED',
  13: 'INTERNAL', 14: 'UNAVAILABLE', 15: 'DATA_LOSS', 16: 'UNAUTHENTICATED',
};

export function grpcStatusLabel(code: number | null): string | null {
  if (code === null) return null;
  const name = GRPC_STATUS[code];
  return name ? `${code} ${name}` : `${code}`;
}

const HTTP_STATUS: Record<number, string> = {
  200: 'OK', 201: 'Created', 202: 'Accepted', 204: 'No Content',
  301: 'Moved Permanently', 302: 'Found', 304: 'Not Modified', 307: 'Temporary Redirect',
  400: 'Bad Request', 401: 'Unauthorized', 403: 'Forbidden', 404: 'Not Found',
  405: 'Method Not Allowed', 409: 'Conflict', 410: 'Gone', 415: 'Unsupported Media Type',
  418: "I'm a teapot", 422: 'Unprocessable Content', 429: 'Too Many Requests',
  500: 'Internal Server Error', 501: 'Not Implemented', 502: 'Bad Gateway',
  503: 'Service Unavailable', 504: 'Gateway Timeout',
};

export function bodyLanguage(headers: Record<string, string>, isJson: boolean): string {
  if (isJson) return 'json';
  const type = Object.entries(headers)
    .find(([k]) => k.toLowerCase() === 'content-type')?.[1]
    ?.toLowerCase() ?? '';
  if (type.includes('html')) return 'html';
  if (type.includes('xml')) return 'xml';
  if (type.includes('json')) return 'json';
  if (type.includes('css')) return 'css';
  if (type.includes('javascript')) return 'javascript';
  return 'plaintext';
}

export function sentHeaders(headers: Record<string, string>): Record<string, string> {
  return Object.fromEntries(Object.entries(headers).filter(([k]) => !k.startsWith(':')));
}

export function httpStatusLabel(code: number | null): string | null {
  if (code === null) return null;
  const name = HTTP_STATUS[code];
  return name ? `${code} ${name}` : `${code}`;
}

export function httpStatusTone(code: number | null): 'ok' | 'warn' | 'fail' | null {
  if (code === null) return null;
  if (code < 300) return 'ok';
  if (code < 400) return 'warn';
  return 'fail';
}

export function byteSize(value: unknown): number {
  if (value === undefined) return 0;
  const text = typeof value === 'string' ? value : JSON.stringify(value) ?? '';
  return new TextEncoder().encode(text).length;
}

export function humanBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} kB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export function durationLabel(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 10_000) return `${(ms / 1000).toFixed(1)} s`;
  return `${Math.round(ms / 1000)} s`;
}

export function timeoutSeconds(ms: number): number {
  return ms > 0 ? Math.ceil(ms / 1000) : 0;
}

export function durationRange(low: number, high: number): string {
  if (low === high) return durationLabel(low);
  const lowLabel = durationLabel(low);
  const highLabel = durationLabel(high);
  const lowUnit = lowLabel.slice(lowLabel.indexOf(' ') + 1);
  const highUnit = highLabel.slice(highLabel.indexOf(' ') + 1);
  if (lowUnit !== highUnit) return `${lowLabel} – ${highLabel}`;
  return `${lowLabel.slice(0, lowLabel.indexOf(' '))}–${highLabel}`;
}

export function jsonProblem(text: string): string | null {
  if (text.trim() === '') return null;
  try {
    JSON.parse(text);
    return null;
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    return message
      .replace(/^JSON\.parse:\s*/, '')
      .replace(/\s*(?:in|at) position \d+/, '')
      .trim();
  }
}

export function jsonStream(text: string): { messages: number; problem: string | null } {
  if (text.trim() === '') return { messages: 0, problem: null };
  if (jsonProblem(text) === null) return { messages: 1, problem: null };

  let messages = 0;
  let buffer = '';
  let lastProblem: string | null = null;
  for (const line of text.split('\n')) {
    buffer = buffer === '' ? line : `${buffer}\n${line}`;
    if (buffer.trim() === '') { buffer = ''; continue; }
    const problem = jsonProblem(buffer);
    if (problem === null) {
      messages += 1;
      buffer = '';
      lastProblem = null;
    } else {
      lastProblem = problem;
    }
  }
  if (buffer.trim() !== '') return { messages, problem: lastProblem ?? jsonProblem(buffer) };
  return { messages, problem: null };
}

export function shortPath(value: string, keep = 34): string {
  const path = value.trim();
  if (path.length <= keep) return path;
  const tail = path.slice(-keep);
  const cut = tail.indexOf('/');
  return `…${cut >= 0 ? tail.slice(cut) : tail}`;
}

export function capLines(text: string, max: number): { shown: string; hidden: number } {
  const lines = text.replace(/\s+$/, '').split('\n');
  if (lines.length <= max) return { shown: lines.join('\n'), hidden: 0 };
  return { shown: lines.slice(0, max).join('\n'), hidden: lines.length - max };
}
