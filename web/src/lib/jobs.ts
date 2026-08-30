import { durationLabel } from './format';
import type { CallResult } from './types';
import { count, plural } from 'luvo/data/plural';

export type JobStatus = 'running' | 'passed' | 'failed' | 'cancelled';

export type JobSummary = {
  id: string;
  reports: string[];
  kind: 'run' | 'bench';
  status: JobStatus;
  paths: string[];
  started_ms: number;
  finished_ms: number | null;
  total: number;
  passed: number;
  failed: number;
  skipped: number;
  duration_ms: number;
};

export type BenchProgress = {
  elapsed_s: number;
  requests: number;
  errors: number;
  rps: number;
  targetRps: number;
  errorPct: number;
};

export type BenchReport = {
  summary: Record<string, number>;
  latency_distribution?: { percentile: number; latency_ns: number }[];
  threshold_evaluation?: { metric: string; expr: string; passed: boolean; actual: string; reason?: string | null }[];
  client_cost?: {
    generator_limited?: boolean;
    limits?: string[];
    cpu_seconds?: number;
    cpu_us_per_request?: number;
    rps_per_core?: number;
    cores_used?: number;
    host_cores?: number;
  } | null;
  levels?: {
    concurrency: number;
    summary: Record<string, number>;
    latency_distribution?: { percentile: number; latency_ns: number }[];
  }[];
  [key: string]: unknown;
};

export type JobEvent = {
  event: 'suite_start' | 'test_start' | 'test_pass' | 'test_fail' | 'test_skip' | 'suite_end'
    | 'bench_progress' | 'bench_report';
  testId?: string;
  interrupted?: boolean;
  extracted?: [string, string][];
  testCount?: number;
  workers?: number;
  coverage?: { covered: number; methods: number; untested: string[] };
  duration?: number;
  grpcDuration?: number;
  grpcStatus?: number;
  message?: string;
  assertions?: { line: number; expression: string; passed: boolean; expected?: unknown; actual?: unknown; message?: string; endpoint?: string; elapsedMs?: number; hint?: string }[];
  documents?: number[];
  response?: {
    messages?: unknown[];
    headers?: Record<string, string>;
    trailers?: Record<string, string>;
    error?: string | null;
  };
  responseStep?: number;
  address?: string;
  summary?: { total: number; passed: number; failed: number; skipped: number; duration: number };
  elapsed_s?: number;
  requests?: number;
  errors?: number;
  rps?: number;
  targetRps?: number;
  errorPct?: number;
  report?: BenchReport;
};

export type Verdict = {
  path: string;
  cases?: { total: number; failed: number };
  caseLabel?: string;
  state: 'running' | 'pass' | 'fail' | 'skip';
  durationMs?: number;
  message?: string;
  assertions?: JobEvent['assertions'];
  documents?: number[];
  response?: JobEvent['response'];
  responseStep?: number;
  address?: string;
  interrupted?: boolean;
  statusCode?: number;
  extracted?: [string, string][];
};

export function benchFailure(run: RunState): string | null {
  if (run.kind !== 'bench' || run.benchReport !== null) return null;
  const failed = Object.values(run.verdicts).find(v => v.state === 'fail');
  return failed?.message?.trim() || null;
}

export function verdictResponse(verdict: Verdict | undefined): CallResult | null {
  if (!verdict || verdict.state !== 'fail') return null;
  return verdictResult(verdict);
}

function stepDuration(verdict: Verdict): number | null {
  const step = verdict.responseStep;
  if (typeof step !== 'number') return null;
  const each = verdict.documents ?? [];
  return typeof each[step] === 'number' ? each[step] : null;
}

export function verdictResult(verdict: Verdict | undefined): CallResult | null {
  if (!verdict || (verdict.state !== 'fail' && verdict.state !== 'pass')) return null;
  const r = verdict.response ?? {};
  return {
    status: verdict.state === 'fail' ? 'error' : 'ok',
    statusCode: verdict.statusCode ?? null,
    messages: r.messages ?? [],
    headers: r.headers ?? {},
    trailers: r.trailers ?? {},
    error: verdict.state === 'fail' ? r.error ?? verdict.message ?? null : null,
    durationMs: stepDuration(verdict) ?? verdict.durationMs ?? null,
    assertions: (verdict.assertions ?? []).map(a => ({
      line: a.line,
      expression: a.expression,
      passed: a.passed,
      elapsed_ms: a.elapsedMs ?? 0,
      message: a.message ?? null,
      expected: a.expected == null ? null : String(a.expected),
      actual: a.actual == null ? null : String(a.actual),
      ...(a.endpoint ? { endpoint: a.endpoint } : {}),
      ...(a.hint ? { hint: a.hint } : {}),
    })),
    fromRun: true,
    ...(typeof verdict.responseStep === 'number' ? { fromStep: verdict.responseStep } : {}),
    ...(verdict.caseLabel ? { fromCase: verdict.caseLabel } : {}),
  };
}

const MAX_TICKS = 600;

export type RunState = {
  kind: 'run' | 'bench';
  benchProgress: BenchProgress | null;
  benchTicks: BenchProgress[];
  benchReport: BenchReport | null;
  total: number;
  done: number;
  passed: number;
  failed: number;
  skipped: number;
  verdicts: Record<string, Verdict>;
  cases: Record<string, Verdict>;
  finished: boolean;
  durationMs: number;
  outcome: JobStatus | null;
  lost: number;
  upToStep?: number;
  workers?: number;
  coverage?: { covered: number; methods: number; untested: string[] };
};

export function untestedNames(coverage: RunState['coverage']): string[] {
  return (coverage?.untested ?? []).map(name => name.replace(/^\w+:\/\//, ''));
}

export function coverageNote(
  coverage: RunState['coverage'],
  shown = 8,
): { label: string; title: string } | null {
  if (!coverage || coverage.methods === 0) return null;
  const names = untestedNames(coverage);
  const head = names.slice(0, shown);
  const rest = names.length - head.length;
  const listed = head.length === 0
    ? 'Every method of the schemas this run dialled was called'
    : ['Never called by this run:', ...head, ...(rest > 0 ? [`and ${rest} more`] : [])].join('\n');
  return {
    label: `methods ${coverage.covered}/${coverage.methods}`,
    title: listed,
  };
}

export const emptyRun = (): RunState => ({
  kind: 'run', benchProgress: null, benchTicks: [], benchReport: null,
  total: 0, done: 0, passed: 0, failed: 0, skipped: 0, verdicts: {}, cases: {}, finished: false, durationMs: 0,
  outcome: null, lost: 0,
});

export function caseNote(run: RunState): { label: string; title: string } | null {
  const files = Object.keys(run.verdicts).length;
  if (files === 0 || run.total <= files) return null;
  return {
    label: `${run.total} cases · ${files} files`,
    title: 'A file with rows runs once per row, so the counts beside this are cases; the tree below is files',
  };
}

export function benchLine(run: RunState): { label: string; title: string } | null {
  if (run.kind !== 'bench') return null;
  const tick = run.benchProgress;
  if (!tick) return { label: 'starting', title: 'The load runner is warming up' };
  const parts = [
    `${Math.round(tick.elapsed_s)} s`,
    `${tick.requests} req`,
    `${Math.round(tick.rps)} rps`,
  ];
  if (tick.errorPct > 0) parts.push(`${tick.errorPct.toFixed(2)}% err`);
  return {
    label: parts.join(' · '),
    title: tick.targetRps > 0
      ? `Asking for ${Math.round(tick.targetRps)} rps · ${tick.errors} of ${tick.requests} came back an error`
      : `${tick.errors} of ${tick.requests} came back an error`,
  };
}

export function runProgressLine(run: RunState): string {
  if (run.lost > 0) return `stream lost · reconnecting (${run.lost})`;
  if (run.kind === 'bench') {
    const tick = run.benchProgress;
    if (!tick) return 'benching';
    return `benching · ${Math.round(tick.elapsed_s)} s · ${tick.requests} req`;
  }
  const total = run.total > 0 ? `/${run.total}` : '';
  const failed = run.failed > 0 ? ` · ${run.failed} failed` : '';
  return `running ${run.done}${total}${failed}`;
}

export function fileOfCase(testId: string): string {
  const at = testId.indexOf('#[row=');
  return at < 0 ? testId : testId.slice(0, at);
}

function rowOfCase(testId: string): string | null {
  const at = testId.indexOf('#[row=');
  return at < 0 ? null : testId.slice(at + 2, testId.length - 1);
}

export function caseTitle(row: string | null, total?: number): string | null {
  if (row === null) return null;
  const at = row.match(/^row=(\d+)\s*(.*)$/s);
  if (!at) return row.trim() === '' ? null : row.trim();
  const nth = Number(at[1]) + 1;
  const of = typeof total === 'number' && total > 0 ? ` of ${total}` : '';
  const bound = at[2].trim();
  return `row ${nth}${of}${bound === '' ? '' : ` · ${bound}`}`;
}

const WORST = { running: 3, fail: 2, skip: 1, pass: 0 } as const;

function fold(path: string, cases: Verdict[]): Verdict {
  if (cases.length === 1) {
    const only = caseTitle(rowOfCase(cases[0].path), 1);
    return { ...cases[0], path, ...(only ? { caseLabel: only } : {}) };
  }
  const worst = cases.reduce((a, b) => (WORST[b.state] > WORST[a.state] ? b : a));
  const failed = cases.filter(c => c.state === 'fail');
  const evidence = failed[0];
  const shown = evidence ?? worst;
  return {
    ...shown,
    path,
    state: worst.state,
    durationMs: cases.reduce((sum, c) => sum + (c.durationMs ?? 0), 0),
    cases: { total: cases.length, failed: failed.length },
    caseLabel: caseTitle(rowOfCase(shown.path), cases.length) ?? undefined,
  };
}

function foldAll(cases: Record<string, Verdict>): Record<string, Verdict> {
  const byFile: Record<string, Verdict[]> = {};
  for (const [testId, verdict] of Object.entries(cases)) {
    (byFile[fileOfCase(testId)] ??= []).push(verdict);
  }
  return Object.fromEntries(Object.entries(byFile).map(([path, list]) => [path, fold(path, list)]));
}

function tally(verdicts: Record<string, Verdict>) {
  const states = Object.values(verdicts).map(v => v.state);
  return {
    done: states.filter(st => st !== 'running').length,
    passed: states.filter(st => st === 'pass').length,
    failed: states.filter(st => st === 'fail').length,
    skipped: states.filter(st => st === 'skip').length,
  };
}

export function applyEvent(state: RunState, e: JobEvent): RunState {
  switch (e.event) {
    case 'suite_start':
      return {
        ...emptyRun(),
        kind: state.kind,
        upToStep: state.upToStep,
        total: e.testCount ?? 0,
        ...(e.workers ? { workers: e.workers } : {}),
      };

    case 'bench_progress': {
      const tick: BenchProgress = {
        elapsed_s: e.elapsed_s ?? 0,
        requests: e.requests ?? 0,
        errors: e.errors ?? 0,
        rps: e.rps ?? 0,
        targetRps: e.targetRps ?? 0,
        errorPct: e.errorPct ?? 0,
      };
      const kept = state.benchTicks.filter(t => t.elapsed_s < tick.elapsed_s);
      const series = [...kept, tick];
      return {
        ...state,
        kind: 'bench',
        benchProgress: tick,
        benchTicks: series.length > MAX_TICKS ? series.slice(series.length - MAX_TICKS) : series,
      };
    }

    case 'bench_report':
      return { ...state, kind: 'bench', benchReport: e.report ?? null };
    case 'test_start': {
      if (!e.testId) return state;
      const cases = { ...state.cases, [e.testId]: { path: e.testId, state: 'running' as const } };
      return { ...state, cases, verdicts: foldAll(cases) };
    }
    case 'test_pass':
    case 'test_fail':
    case 'test_skip': {
      if (!e.testId) return state;
      const kind = e.event === 'test_pass' ? 'pass' : e.event === 'test_fail' ? 'fail' : 'skip';
      const cases = {
        ...state.cases,
        [e.testId]: {
          path: e.testId,
          state: kind,
          durationMs: e.duration,
          message: e.message,
          assertions: e.assertions,
          documents: e.documents,
          response: e.response,
          ...(typeof e.responseStep === 'number' ? { responseStep: e.responseStep } : {}),
          address: e.address,
          ...(typeof e.grpcStatus === 'number' ? { statusCode: e.grpcStatus } : {}),
          ...(e.interrupted ? { interrupted: true } : {}),
          ...(e.extracted && e.extracted.length > 0 ? { extracted: e.extracted } : {}),
        } as Verdict,
      };
      return { ...state, ...tally(cases), cases, verdicts: foldAll(cases) };
    }
    case 'suite_end':
      return {
        ...state,
        finished: true,
        durationMs: e.summary?.duration ?? state.durationMs,
        passed: e.summary?.passed ?? state.passed,
        failed: e.summary?.failed ?? state.failed,
        skipped: e.summary?.skipped ?? state.skipped,
        ...(e.coverage ? { coverage: e.coverage } : {}),
      };
    default:
      return state;
  }
}

export function scopeFiles(all: string[], scope: 'file' | 'folder' | 'all', current: string | null): string[] {
  if (scope === 'all') return all;
  if (!current) return [];
  if (scope === 'file') return [current];
  const dir = current.split('/').slice(0, -1).join('/');
  return all.filter(p => p.split('/').slice(0, -1).join('/') === dir);
}

export function unsavedAmong(targets: string[], open: { path: string | null; dirty: boolean }[]): string[] {
  const wanted = new Set(targets);
  const found = new Set<string>();
  for (const tab of open) {
    if (tab.dirty && tab.path !== null && wanted.has(tab.path)) found.add(tab.path);
  }
  return [...found];
}

export interface DataFile {
  path: string;
  name: string;
  size: number;
  format: 'csv' | 'tsv' | 'ndjson';
  columns?: string[];
}

export async function dataFiles(): Promise<DataFile[]> {
  try {
    const res = await fetch('/api/data-files');
    return res.ok ? await res.json() : [];
  } catch {
    return [];
  }
}

export interface RunRefusal {
  text: string;
  path: string | null;
}

export function runRefusal(said: string, openPaths: string[]): RunRefusal {
  const text = said.trim();
  const gone = text.match(/^File not found: (.+)$/);
  if (!gone) return { text, path: null };
  const path = gone[1].trim();
  return {
    text: openPaths.includes(path)
      ? `${path} is not on disk any more — Save writes this tab back to it`
      : `${path} is not on disk any more — it was renamed or deleted since the rail read it`,
    path,
  };
}

export async function startJob(
  paths: string[],
  upToStep?: number,
  kind: 'run' | 'bench' = 'run',
  reports: string[] = [],
  data?: string | null,
): Promise<JobSummary> {
  const res = await fetch('/api/jobs', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ kind, paths, up_to_step: upToStep, reports, data: data || undefined }),
  });
  if (!res.ok) throw new Error(await res.text());
  return res.json();
}

export async function cancelJob(id: string): Promise<void> {
  await fetch(`/api/jobs/${id}/cancel`, { method: 'POST' });
}

export async function jobSummary(id: string): Promise<JobSummary | null> {
  try {
    const res = await fetch(`/api/jobs/${id}`);
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

export async function runningJobs(): Promise<JobSummary[]> {
  try {
    const res = await fetch('/api/jobs');
    if (!res.ok) return [];
    const jobs: JobSummary[] = await res.json();
    return jobs.filter(j => j.status === 'running');
  } catch {
    return [];
  }
}

const RECONNECTS = 5;
const RECONNECT_MS = 1000;

export function followJob(
  id: string,
  onEvent: (e: JobEvent) => void,
  onClose?: (final: JobSummary | null) => void,
  onLost?: (attempt: number) => void,
): () => void {
  let stopped = false;
  let attempts = 0;
  let source: EventSource | null = null;

  const open = () => {
    source = new EventSource(`/api/jobs/${id}/events`);
    source.onmessage = m => {
      attempts = 0;
      try { onEvent(JSON.parse(m.data)); } catch { /* a malformed frame is not worth a crash */ }
    };
    source.onerror = () => {
      source?.close();
      if (stopped) return;
      void (async () => {
        const summary = await jobSummary(id);
        if (stopped) return;
        if (summary?.status !== 'running' || attempts >= RECONNECTS) {
          stopped = true;
          onClose?.(summary);
          return;
        }
        attempts += 1;
        onLost?.(attempts);
        setTimeout(() => { if (!stopped) open(); }, RECONNECT_MS * attempts);
      })();
    };
  };

  open();
  return () => { stopped = true; source?.close(); };
}

export function rollUp(paths: string[], verdicts: Record<string, Verdict>) {
  let passed = 0, failed = 0, skipped = 0, running = 0;
  for (const p of paths) {
    const v = verdicts[p];
    if (!v) continue;
    if (v.state === 'pass') passed++;
    else if (v.state === 'fail') failed++;
    else if (v.state === 'skip') skipped++;
    else running++;
  }
  return { passed, failed, skipped, running, touched: passed + failed + skipped + running };
}

export function moreRowsNote(v: Verdict): string | null {
  const failed = v.cases?.failed ?? 0;
  if (failed < 2) return null;
  return `+${failed - 1} more row${failed === 2 ? '' : 's'} failed`;
}

export function slowNote(durationMs: number | undefined, floorMs = 1000): string | null {
  if (durationMs === undefined || durationMs < floorMs) return null;
  return durationLabel(durationMs);
}

export function verdictLabel(v: Verdict, upToStep?: number): string {
  if (v.state === 'running') return '…';
  if (upToStep !== undefined && v.state !== 'skip') {
    return upToStep === 1 ? 'step 1' : `steps 1–${upToStep}`;
  }
  if (v.cases) {
    if (v.state === 'fail') return `${v.cases.failed}/${v.cases.total} ${plural(v.cases.total, 'row')}`;
    if (v.state === 'skip') return v.interrupted ? 'cancelled' : 'not run';
    return count(v.cases.total, 'row');
  }
  if (v.state === 'skip') return v.interrupted ? 'cancelled' : 'not run';
  if (v.state === 'fail') {
    const total = v.assertions?.length ?? 0;
    if (total > 0) {
      const passed = v.assertions!.filter(a => a.passed).length;
      if (passed < total) return `checks ${passed}/${total}`;
    }
    return 'failed';
  }
  return v.durationMs !== undefined ? durationLabel(v.durationMs) : 'passed';
}

export function failureHeadline(message: string): string {
  const lines = message.split('\n').map(l => l.trim()).filter(Boolean);
  const detail = lines.find(l => l.startsWith('-'));
  if (detail) return detail.replace(/^-\s*/, '');
  const [head, ...rest] = lines;
  if (head === undefined) return message;
  if (!head.endsWith(':') || rest.length === 0) return head;
  const kind = head.replace(/:\s*$/, '').split(':')[0].trim();
  const reason = rest[0];
  return kind ? `${kind}: ${reason}` : reason;
}

export type StepMark = { state: 'pass' | 'fail' | 'skip' | 'none'; durationMs?: number };

export function stepMarks(verdict: Verdict | undefined, steps: number): StepMark[] {
  const marks: StepMark[] = Array.from({ length: steps }, () => ({ state: 'none' as const }));
  if (!verdict || verdict.state === 'running') return marks;

  const durations = verdict.documents ?? [];
  const ran = Math.min(durations.length, steps);
  for (let i = 0; i < ran; i++) marks[i] = { state: 'pass', durationMs: durations[i] };

  if (verdict.state === 'fail' && steps > 0) {
    const failedAt = ran > 0 ? ran - 1 : 0;
    marks[failedAt] = { state: 'fail', durationMs: durations[failedAt] };
    for (let i = failedAt + 1; i < steps; i++) marks[i] = { state: 'skip' };
  }
  return marks;
}

export function failureLine(verdict: Verdict): { text: string; detail: string | null; line: number | null } | null {
  const failed = (verdict.assertions ?? []).filter(a => !a.passed);
  if (failed.length > 0) {
    const a = failed[0];
    const pair = a.expected !== undefined && a.actual !== undefined;
    const blocky = pair && (String(a.expected).includes('\n') || String(a.actual).includes('\n'));
    const detail = pair && !blocky
      ? `expected ${format(a.expected)}, got ${format(a.actual)}`
      : a.message ?? null;
    return { text: railText(a.expression), detail: clip(detail), line: a.line ?? null };
  }
  if (verdict.message) return { text: failureHeadline(verdict.message), detail: null, line: null };
  return null;
}

function railText(expression: string): string {
  const section = expression.match(/^---\s*(.+?)\s*---$/);
  return section ? section[1] : expression;
}

function clip(text: string | null, max = 90): string | null {
  if (text === null) return null;
  const oneLine = text.replace(/\s+/g, ' ').trim();
  return oneLine.length > max ? `${oneLine.slice(0, max - 1)}…` : oneLine;
}

function format(value: unknown): string {
  if (typeof value === 'string') return JSON.stringify(value);
  return String(value);
}

export async function jobReports(id: string): Promise<string[]> {
  try {
    const res = await fetch(`/api/jobs/${id}`);
    if (!res.ok) return [];
    const job = await res.json();
    return job.reports ?? [];
  } catch {
    return [];
  }
}
