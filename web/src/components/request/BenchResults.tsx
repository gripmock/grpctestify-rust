import { useEffect, useMemo, type ReactNode } from 'react';
import { isTabDirty, useStore } from '../../lib/store';
import { benchFailure, type BenchReport } from '../../lib/jobs';
import { capLines, durationLabel } from '../../lib/format';
import { stoppedShort, latencyNote } from '../../lib/bench-model';
import { benchPlot } from '../../lib/bench-plot';
import { Play, Square, TriangleAlert, Layers } from 'lucide-react';
import { count, plural } from 'luvo/data/plural';

const NS = 1_000_000;

export function BenchLauncher({ children }: { children?: ReactNode }) {
  const workspacePath = useStore(s => s.workspacePath);
  const run = useStore(s => s.run);
  const runJobId = useStore(s => s.runJobId);
  const startBench = useStore(s => s.startBench);
  const cancelRun = useStore(s => s.cancelRun);
  const running = runJobId !== null && !run.finished;
  const dirty = useStore(s => {
    const tab = s.tabs.find(t => t.id === s.activeTabId);
    return tab ? isTabDirty(tab) : false;
  });
  const visibleFiles = useStore(s => s.visibleFiles);
  const runError = useStore(s => s.runError);
  const refusedHere = benchFailure(run);

  return (
    <div className="stack">
      {refusedHere && !runError && <Refusal text={refusedHere} />}
      {runError && (
        <div className="assert is-fail">
          <span className="assert-mark">!</span>
          <span>{runError}</span>
        </div>
      )}
      <div className="bar">
      {children}
      <span className="grow" />
      {dirty && <span className="warn">unsaved — a bench reads the file on disk</span>}
      {running && run.kind === 'bench' ? (
        <button className="btn is-sm is-danger" onClick={cancelRun}>
          <Square size={11} /> cancel
        </button>
      ) : (
        <span className="btn-split">
          <button
            className="btn is-sm is-primary"
            disabled={!workspacePath || running}
            onClick={() => workspacePath && startBench(workspacePath)}
            title={!workspacePath
              ? 'Save the file first — a bench reads it from disk'
              : dirty
                ? 'Runs the file as it is on disk — the edits open here are not in it. Save first to measure them.'
                : 'Run this file’s BENCH section — the same measurement grpctestify bench takes'}
          >
            <Play size={11} /> bench
          </button>
          <button
            className="btn is-sm is-primary is-icon"
            disabled={visibleFiles.length < 2 || running}
            onClick={() => startBench(visibleFiles)}
            title={`Bench all ${count(visibleFiles.length, 'file')} the rail is showing, as one measurement`}
          >
            <Layers size={11} />
          </button>
        </span>
      )}
      </div>
    </div>
  );
}

function Refusal({ text }: { text: string }) {
  const { shown, hidden } = capLines(text, 6);
  return (
    <div className="assert is-fail" title={hidden > 0 ? text : undefined}>
      <span className="assert-mark">!</span>
      <span className="pre">
        {shown}
        {hidden > 0 && <span className="muted">{`\n…and ${hidden} more ${plural(hidden, 'line')} — hover to read them all`}</span>}
      </span>
    </div>
  );
}

export function BenchResults() {
  const run = useStore(s => s.run);
  const runError = useStore(s => s.runError);
  const runJobId = useStore(s => s.runJobId);
  const running = runJobId !== null && !run.finished;
  const isBench = run.kind === 'bench';
  const tick = isBench ? run.benchProgress : null;
  const report = isBench ? run.benchReport : null;
  const summary = report?.summary ?? {};
  const axesDiffer = summary.ok !== summary.passed || summary.errors !== summary.failed;
  const ticks = run.benchTicks;
  const plot = useMemo(() => (isBench ? benchPlot(ticks) : null), [isBench, ticks]);
  const refused = benchFailure(run);
  const overUnsaved = useStore(s => s.benchOverUnsaved);
  const planned = useStore(s => s.collectionParsed?.bench?.duration);
  const stopped = stoppedShort(run.outcome, run.durationMs, planned);

  return (
    <div className="stack">
      {refused && !runError && <Refusal text={refused} />}
      {runError && (
        <div className="assert is-fail">
          <span className="assert-mark">!</span>
          <span>{runError}</span>
        </div>
      )}

      {tick && !report && (
        <div className="summary">
          {running && <span className="spinner" />}
          <span className="mono">t={tick.elapsed_s.toFixed(1)}s</span>
          <span className="badge">{tick.requests.toLocaleString()} req</span>
          <span className="badge is-ok">{tick.rps.toFixed(0)} rps</span>
          {tick.targetRps > 0 && <span className="muted mono">target {tick.targetRps.toFixed(0)}</span>}
          <span className={`badge ${tick.errorPct > 0 ? 'is-fail' : ''}`}>
            {tick.errorPct.toFixed(2)}% errors
          </span>
        </div>
      )}

      {report && stopped && (
        <div className="note is-warn bench-stale">
          <TriangleAlert size={11} />
          <span>{stopped}</span>
        </div>
      )}

      {report && overUnsaved.length > 0 && (
        <div className="note is-warn bench-stale">
          <TriangleAlert size={11} />
          <span>
            Measured the {overUnsaved.length === 1 ? 'file' : 'files'} as{' '}
            {overUnsaved.length === 1 ? 'it was' : 'they were'} on disk —{' '}
            <span className="mono">{overUnsaved.join(', ')}</span> had unsaved edits when this ran.
          </span>
        </div>
      )}

      {plot && (
        <div className="stack is-tight">
          <div className="plot">
            <svg viewBox="0 0 300 100" preserveAspectRatio="none" role="img" aria-label="observed rate over time">
              <line className="grid" x1="0" y1="25" x2="300" y2="25" />
              <line className="grid" x1="0" y1="50" x2="300" y2="50" />
              <line className="grid" x1="0" y1="75" x2="300" y2="75" />
              {plot.target && <polyline className="target" points={plot.target} />}
              <polyline className="series" points={plot.observed} />
            </svg>
          </div>
          <div className="plot-legend">
            <span><i className="is-series" />observed</span>
            {plot.target && <span><i className="is-target" />target</span>}
            <span className="grow" />
            <span className="muted mono">peak {fmt(plot.peak)} rps · {plot.span.toFixed(1)} s · {count(ticks.length, 'sample')}</span>
          </div>
        </div>
      )}

      {report && (
        <>
          <div className="tiles">
            <Tile label="rps" value={fmt(summary.rps_observed)} />
            <Tile label="p95" value={ms(percentile(report, 95))} />
            <Tile label="p99" value={ms(percentile(report, 99))} />
            <Tile label="passed" value={fmt(summary.passed)} />
            <Tile label="failed" value={fmt(summary.failed)} tone={summary.failed ? 'fail' : undefined} />
            {axesDiffer && (
              <Tile label="ok / errors" value={`${fmt(summary.ok)} / ${fmt(summary.errors)}`} />
            )}
          </div>

          {(report.latency_distribution ?? []).length > 0 && (
            <div className="stack is-hair">
              <span className="label">
                latency, as measured
                {latencyNote(report.summary) && (
                  <span className="warn"> · {latencyNote(report.summary)}</span>
                )}
              </span>
              <div className="bar wrap latency-row">
                {report.latency_distribution!.map(p => (
                  <span key={p.percentile} className="latency-cell">
                    <span className="muted">p{trimPercentile(p.percentile)}</span>
                    <span className="mono">{ms(p.latency_ns / NS)}</span>
                  </span>
                ))}
              </div>
            </div>
          )}

          {report.client_cost?.generator_limited && (
            <div className="note is-warn">
              <TriangleAlert size={11} /> generator-limited: the client was the ceiling, so this
              number is ours, not the target’s.
              {report.client_cost.limits?.length ? ` ${report.client_cost.limits.join(' · ')}` : ''}
            </div>
          )}

          {report.client_cost && (
            <div className="stack is-hair">
              <span className="label">client cost</span>
              <dl className="kv">
                {report.client_cost.cpu_us_per_request !== undefined && (
                  <div className="bar"><dt>cpu / request</dt><dd className="mono">{report.client_cost.cpu_us_per_request.toFixed(1)} µs</dd></div>
                )}
                {report.client_cost.cores_used !== undefined && (
                  <div className="bar">
                    <dt>cores used</dt>
                    <dd className="mono">
                      {report.client_cost.cores_used.toFixed(2)}
                      {report.client_cost.host_cores !== undefined ? ` / ${report.client_cost.host_cores}` : ''}
                    </dd>
                  </div>
                )}
                {report.client_cost.rps_per_core !== undefined && (
                  <div className="bar"><dt>rps / core</dt><dd className="mono">{fmt(report.client_cost.rps_per_core)}</dd></div>
                )}
              </dl>
            </div>
          )}

          {(report.threshold_evaluation ?? []).length > 0 && (
            <div className="stack is-hair">
              <span className="label">thresholds</span>
              {report.threshold_evaluation!.map(t => (
                <div key={t.metric} className={`assert ${t.passed ? 'is-ok' : 'is-fail'}`}>
                  <span className="assert-mark">{t.passed ? '✓' : '✗'}</span>
                  <span className="mono grow">
                    {t.metric} {t.expr}
                    {t.reason && <span className="muted"> — {t.reason}</span>}
                  </span>
                  <span className="muted mono">{t.actual}</span>
                </div>
              ))}
              <span className="muted">
                A failed threshold exits 1 in CI — the report is still written.
              </span>
            </div>
          )}

          {(report.levels ?? []).length > 0 && (
            <div className="stack is-hair">
              <span className="label">concurrency sweep</span>
              <div className="matches">
                {report.levels!.map(l => {
                  const p95 = l.latency_distribution?.find(p => Math.abs(p.percentile - 95) < 0.001);
                  return (
                    <div key={l.concurrency} className="match-row">
                      <span className="mono sweep-c">c={l.concurrency}</span>
                      <span className="mono grow">{fmt(l.summary.rps_observed)} rps</span>
                      {p95 && <span className="muted mono">{ms(p95.latency_ns / NS)} p95</span>}
                      <span className="muted mono">{fmt(l.summary.errors)} err</span>
                    </div>
                  );
                })}
              </div>
            </div>
          )}

          <BenchCompare />

          <span className="muted">
            measured in {durationLabel(run.durationMs)}
            {axesDiffer
              && ' · transport status and document verdict are different axes — a negative-path bench passes while every request is non-OK'}
          </span>
        </>
      )}
    </div>
  );
}

function BenchCompare() {
  const baseline = useStore(s => s.benchBaseline);
  const report = useStore(s => s.run.benchReport);
  const comparison = useStore(s => s.benchComparison);
  const compareBench = useStore(s => s.compareBench);
  const partial = useStore(s => s.run.outcome === 'cancelled' || s.benchBaselinePartial);

  useEffect(() => {
    if (baseline && report && !comparison) void compareBench();
  }, [baseline, report, comparison, compareBench]);

  if (!comparison || comparison.metrics.length === 0) return null;

  return (
    <div className="stack is-hair">
      <div className="bar">
        <span className="label grow">compare with the previous run</span>
        {partial
          ? <span className="badge is-pending" title="One of these runs was stopped by hand, so the two measured different lengths of time">not like for like</span>
          : comparison.overall === 'fail' && <span className="badge is-fail">regressed</span>}
      </div>
      <div className="matches">
        <div className="match-row muted">
          <span className="cmp-metric">metric</span>
          <span className="cmp-value">base</span>
          <span className="cmp-value">now</span>
          <span className="grow">Δ</span>
        </div>
        {comparison.metrics.map(row => (
          <div key={row.name} className="match-row">
            <span className="mono cmp-metric">{metricName(row.name)}</span>
            <span className="mono muted cmp-value">{metricValue(row.name, row.baseline)}</span>
            <span className="mono cmp-value">{metricValue(row.name, row.current)}</span>
            <span className={`mono grow${row.verdict === 'regressed' ? ' fail' : ''}`}>
              {row.pct_delta === null ? '—' : `${row.pct_delta > 0 ? '+' : ''}${row.pct_delta.toFixed(1)}%`}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

const num = (n: number) => (Math.abs(n) >= 1000 ? Math.round(n).toLocaleString() : n.toFixed(2));

function metricName(name: string) {
  return name.replace(/^latency_/, '').replace(/_rps$/, '').replace(/_/g, ' ');
}

function metricValue(name: string, value: number) {
  return name.startsWith('latency_') ? ms(value / NS) : num(value);
}

function Tile({ label, value, tone }: { label: string; value: string; tone?: 'fail' }) {
  return (
    <div className="tile">
      <span className="k">{label}</span>
      <span className={`v${tone === 'fail' ? ' is-fail' : ''}`}>{value}</span>
    </div>
  );
}

function percentile(report: BenchReport, want: number) {
  const hit = report.latency_distribution?.find(p => Math.abs(p.percentile - want) < 0.001);
  return hit ? hit.latency_ns / NS : undefined;
}

function trimPercentile(p: number): string {
  return Number.isInteger(p) ? String(p) : String(Number(p.toFixed(2)));
}

const fmt = (n: number | undefined) => (n === undefined ? '—' : Math.round(n).toLocaleString());
const ms = (n: number | undefined) => (n === undefined ? '—' : `${n.toFixed(1)} ms`);
