export type Field =
  | { kind: 'number'; key: string; label: string; hint?: string }
  | { kind: 'duration'; key: string; label: string; hint?: string }
  | { kind: 'enum'; key: string; label: string; values: readonly string[]; hint?: string }
  | { kind: 'bool'; key: string; label: string; hint?: string }
  | { kind: 'text'; key: string; label: string; hint?: string };

export type Group = { title: string; fields: Field[] };

export const MODES = ['fixed', 'stepping', 'adaptive', 'closed', 'open'] as const;
const LOAD_SCHEDULES = ['const', 'step', 'line', 'sine', 'spike', 'custom'] as const;
const CONCURRENCY_SCHEDULES = ['const', 'step', 'line'] as const;
const ASSERT_MODES = ['full', 'sampled', 'off', 'fail_fast', 'collect_all', 'skip'] as const;
const DURATION_STOPS = ['close', 'wait', 'ignore'] as const;

export const BENCH_METRICS = [
  'rps',
  'count',
  'errors',
  'passed',
  'failed',
  'pass_rate_pct',
  'fail_rate_pct',
  'error_rate_pct',
  'average_ms',
  'fastest_ms',
  'slowest_ms',
  'latency_ms.p50',
  'latency_ms.p90',
  'latency_ms.p95',
  'latency_ms.p99',
] as const;

export const BENCH_GROUPS: Group[] = [
  {
    title: 'shape',
    fields: [
      { kind: 'enum', key: 'mode', label: 'mode', values: MODES },
      { kind: 'number', key: 'concurrency', label: 'concurrency' },
      { kind: 'number', key: 'connections', label: 'connections' },
      { kind: 'number', key: 'requests', label: 'requests', hint: 'total budget, split across documents' },
      { kind: 'duration', key: 'duration', label: 'duration' },
      { kind: 'number', key: 'max_rps', label: 'max rps' },
    ],
  },
  {
    title: 'schedule',
    fields: [
      { kind: 'enum', key: 'load_schedule', label: 'load schedule', values: LOAD_SCHEDULES },
      { kind: 'number', key: 'load_start', label: 'load start' },
      { kind: 'number', key: 'load_step', label: 'load step' },
      { kind: 'number', key: 'load_end', label: 'load end' },
      { kind: 'duration', key: 'load_step_duration', label: 'step duration' },
      { kind: 'enum', key: 'concurrency_schedule', label: 'concurrency schedule', values: CONCURRENCY_SCHEDULES, hint: 'the only mode that produces levels[]' },
      { kind: 'number', key: 'concurrency_start', label: 'from' },
      { kind: 'number', key: 'concurrency_end', label: 'to' },
      { kind: 'number', key: 'concurrency_step', label: 'step' },
    ],
  },
  {
    title: 'timing',
    fields: [
      { kind: 'duration', key: 'warmup', label: 'warmup' },
      { kind: 'duration', key: 'ramp_up', label: 'ramp up' },
      { kind: 'duration', key: 'cool_down', label: 'cool down' },
      { kind: 'duration', key: 'request_timeout', label: 'request timeout' },
      { kind: 'duration', key: 'connect_timeout', label: 'connect timeout' },
      { kind: 'duration', key: 'progress_interval', label: 'progress interval' },
      { kind: 'enum', key: 'duration_stop', label: 'on stop', values: DURATION_STOPS },
    ],
  },
  {
    title: 'assertions',
    fields: [
      { kind: 'enum', key: 'assert_mode', label: 'assert mode', values: ASSERT_MODES },
      { kind: 'bool', key: 'no_assert', label: 'no assert' },
      { kind: 'number', key: 'sample_rate', label: 'sample rate', hint: '0 to 1' },
    ],
  },
  {
    title: 'reporting',
    fields: [
      { kind: 'text', key: 'name', label: 'name' },
      { kind: 'text', key: 'latency_percentiles', label: 'percentiles', hint: 'p50,p95,p99' },
      { kind: 'text', key: 'sources', label: 'sources', hint: 'data files, comma-separated' },
    ],
  },
];

const DURATION = /^\d+(\.\d+)?(ms|s|m|h)?$/;

export function validDuration(raw: string): boolean {
  return raw.trim() === '' || DURATION.test(raw.trim());
}

export function validNumber(raw: string): boolean {
  const t = raw.trim().replace(/_/g, '');
  return t === '' || (/^\d+(\.\d+)?$/.test(t) && Number.isFinite(Number(t)));
}

const THRESHOLD = /^(<=|>=|<|>)\s*\d+(\.\d+)?$/;

export function validThreshold(raw: string): boolean {
  return THRESHOLD.test(raw.trim());
}

export function thresholdsOf(bench: Record<string, string>): [string, string][] {
  return Object.entries(bench).filter(([k]) => k === 'thresholds' || k.startsWith('thresholds.'));
}

export function isThresholdKey(key: string): boolean {
  return key === 'thresholds' || key.startsWith('thresholds.');
}

export function benchKnownKeys(): string[] {
  return BENCH_GROUPS.flatMap(g => g.fields.map(f => f.key));
}

export function groupApplies(group: Group, bench: Record<string, string>): boolean {
  if (group.title !== 'schedule') return true;
  const mode = (bench.mode ?? '').trim() || 'fixed';
  if (mode !== 'fixed') return true;
  return group.fields.some(field => (bench[field.key] ?? '').trim() !== '');
}

export function stopCondition(bench: Record<string, string>): {
  governs: 'duration' | 'requests' | 'none';
  ignored: 'requests' | null;
} {
  const duration = (bench.duration ?? '').trim();
  const requests = (bench.requests ?? '').trim();
  if (duration !== '') return { governs: 'duration', ignored: requests !== '' ? 'requests' : null };
  if (requests !== '') return { governs: 'requests', ignored: null };
  return { governs: 'none', ignored: null };
}

export function latencyNote(summary: Record<string, number> | undefined): string | null {
  const count = summary?.count ?? 0;
  const ok = summary?.ok ?? 0;
  if (count === 0) return null;
  return ok === 0 ? 'over failed requests only — nothing succeeded' : null;
}

export function fieldsInUse(group: Group, bench: Record<string, string>, opened: string[]): Field[] {
  return group.fields.filter(field =>
    (bench[field.key] ?? '').trim() !== '' || opened.includes(field.key));
}

export function fieldsToAdd(bench: Record<string, string>, opened: string[]): Group[] {
  return BENCH_GROUPS
    .map(group => ({
      title: group.title,
      fields: group.fields.filter(field =>
        (bench[field.key] ?? '').trim() === '' && !opened.includes(field.key)),
    }))
    .filter(group => group.fields.length > 0);
}

export function stoppedShort(
  outcome: string | null,
  ranMs: number,
  planned?: string,
): string | null {
  if (outcome !== 'cancelled') return null;
  const ran = `${(Math.max(0, ranMs) / 1000).toFixed(1)} s`;
  const asked = (planned ?? '').trim();
  return asked === '' || !validDuration(asked)
    ? `Stopped before it finished — these numbers are the ${ran} it got through, not the run that was asked for.`
    : `Stopped after ${ran} of a ${asked} plan — these numbers are what it got through, not the run that was asked for.`;
}
