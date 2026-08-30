import { explainFailure } from './failure';
import { durationRange } from './format';

export interface DayGroup<T> {
  key: string;
  label: string;
  entries: T[];
}

const MONTHS = ['jan', 'feb', 'mar', 'apr', 'may', 'jun', 'jul', 'aug', 'sep', 'oct', 'nov', 'dec'];

function dayKey(d: Date): string {
  return `${d.getFullYear()}-${d.getMonth()}-${d.getDate()}`;
}

export function groupByDay<T extends { timestamp: number }>(entries: T[], now: number): DayGroup<T>[] {
  const today = dayKey(new Date(now));
  const yesterday = dayKey(new Date(now - 86_400_000));
  const nowYear = new Date(now).getFullYear();

  const groups: DayGroup<T>[] = [];
  for (const entry of entries) {
    const d = new Date(entry.timestamp);
    const key = dayKey(d);
    const last = groups[groups.length - 1];
    if (last && last.key === key) {
      last.entries.push(entry);
      continue;
    }
    const label =
      key === today ? 'today'
      : key === yesterday ? 'yesterday'
      : `${d.getDate()} ${MONTHS[d.getMonth()]}${d.getFullYear() === nowYear ? '' : ` ${d.getFullYear()}`}`;
    groups.push({ key, label, entries: [entry] });
  }
  return groups;
}

export function methodOf(endpoint: string): string {
  const http = /^([A-Za-z-]{3,24})\s+(\S+)$/.exec(endpoint.trim());
  if (http) {
    const path = http[2].replace(/^https?:\/\/[^/]+/, '');
    return `${http[1].toUpperCase()} ${path || '/'}`;
  }
  const tail = endpoint.split('/').filter(Boolean).pop();
  return tail || endpoint;
}

export function serviceOf(endpoint: string): string {
  const parts = endpoint.split('/').filter(Boolean);
  return parts.length > 1 ? parts.slice(0, -1).join('/') : '';
}

export interface Burst<T> {
  key: string;
  entries: T[];
}

export function burstKey(entry: {
  endpoint: string;
  bodies: string[];
  response: { status: string };
  connection?: { address?: string; protocol?: string; tls?: boolean } | null;
  resolved?: string[];
  datasetRow?: number;
}): string {
  const where = entry.connection
    ? `${entry.connection.address ?? ''}|${entry.connection.protocol ?? ''}|${entry.connection.tls ? 'tls' : ''}`
    : '';
  const sent = [...(entry.resolved ?? [])].sort().join(',');
  const row = entry.datasetRow === undefined ? '' : String(entry.datasetRow);
  return `${entry.response.status}|${entry.endpoint}|${entry.bodies.join('\u0000')}|${where}|${sent}|${row}`;
}

export function burstRepeats<T>(entries: T[], keyOf: (entry: T) => string): Burst<T>[] {
  const out: Burst<T>[] = [];
  for (const entry of entries) {
    const key = keyOf(entry);
    const last = out[out.length - 1];
    if (last && last.key === key) last.entries.push(entry);
    else out.push({ key, entries: [entry] });
  }
  return out;
}

export function tookRange(values: (number | null | undefined)[]): string | null {
  const known = values.filter((v): v is number => typeof v === 'number');
  if (known.length === 0) return null;
  return durationRange(Math.min(...known), Math.max(...known));
}

export function payloadPreview(bodies: string[], max = 44): string {
  const first = (bodies ?? []).find(b => (b ?? '').trim() !== '');
  if (!first) return '';
  let compact: string;
  try {
    compact = JSON.stringify(JSON.parse(first));
  } catch {
    compact = first.replace(/\s+/g, ' ').trim();
  }
  if (compact === '{}' ) return '{}';
  return compact.length > max ? `${compact.slice(0, max - 1)}…` : compact;
}

export function dayMark(timestamp: number, now: number): string | null {
  const key = dayKey(new Date(timestamp));
  if (key === dayKey(new Date(now))) return null;
  if (key === dayKey(new Date(now - 86_400_000))) return 'yesterday';
  const d = new Date(timestamp);
  const year = d.getFullYear() === new Date(now).getFullYear() ? '' : ` ${d.getFullYear()}`;
  return `${d.getDate()} ${MONTHS[d.getMonth()]}${year}`;
}

export type CallLine = { text: string; from: 'request' | 'response' | 'error' };

export function callSummary(entry: {
  bodies: string[];
  response: { status: string; error?: string | null; messages?: unknown[]; statusCode?: number | null };
  connection?: { address: string };
}): CallLine {
  if (entry.response.status !== 'ok') {
    const failure = explainFailure(
      entry.response.error ?? '',
      entry.response.statusCode ?? null,
      entry.connection?.address ?? null,
    );
    const reason = failure.title.split('\n')[0].trim();
    return { text: reason || 'failed', from: 'error' };
  }
  const sent = payloadPreview(entry.bodies);
  if (sent && sent !== '{}') return { text: sent, from: 'request' };

  const back = entry.response.messages?.[0];
  if (back !== undefined) {
    const text = payloadPreview([JSON.stringify(back)]);
    if (text && text !== '{}') return { text, from: 'response' };
  }
  return { text: sent || '{}', from: 'request' };
}
