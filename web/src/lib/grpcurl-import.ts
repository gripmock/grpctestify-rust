import { count } from 'luvo/data/plural';
export type ImportedCommand = {
  endpoint: string;
  address: string;
  headers: Record<string, string>;
  body: string;
  plaintext: boolean;
  tls?: Record<string, string>;
  proto?: Record<string, string>;
  options?: Record<string, string>;
};

const OPTION_KEYS: Record<string, string> = {
  'max-time': 'timeout',
  compression: 'compression',
};

export type ImportPlan = {
  options: Record<string, string>;
  adjusted: string[];
  ignored: string[];
};

export function planImport(imported: ImportedCommand): ImportPlan {
  const options: Record<string, string> = {};
  const ignored: string[] = [];
  const adjusted: string[] = [];
  for (const [key, value] of Object.entries(imported.options ?? {})) {
    if (key === 'plaintext') continue;
    const mapped = OPTION_KEYS[key];
    if (!mapped) {
      ignored.push(value === 'true' ? `-${key}` : `-${key} ${value}`);
      continue;
    }
    if (mapped === 'timeout') {
      const seconds = Number(value);
      if (Number.isFinite(seconds) && seconds > 0 && !Number.isInteger(seconds)) {
        const whole = String(Math.ceil(seconds));
        options[mapped] = whole;
        adjusted.push(`-${key} ${value} → timeout: ${whole}s (whole seconds)`);
        continue;
      }
    }
    options[mapped] = value;
  }
  return { options, ignored: ignored.sort(), adjusted };
}

export function importSummary(imported: ImportedCommand, plan: ImportPlan): string {
  const parts: string[] = [];
  const headers = Object.keys(imported.headers ?? {}).length;
  if (headers > 0) parts.push(`${count(headers, 'header')}`);
  if (Object.keys(imported.tls ?? {}).length > 0) parts.push('TLS');
  if (Object.keys(imported.proto ?? {}).length > 0) parts.push('PROTO');
  if (Object.keys(plan.options).length > 0) parts.push('OPTIONS');
  return parts.length > 0 ? `with ${parts.join(', ')}` : '';
}
