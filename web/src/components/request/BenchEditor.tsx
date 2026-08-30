import { useEffect, useState } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { Popover } from 'luvo/ui/Popover';
import { useDismiss } from 'luvo/input/useDismiss';
import { useStore } from '../../lib/store';
import { BENCH_GROUPS, BENCH_METRICS, benchKnownKeys, fieldsInUse, fieldsToAdd, groupApplies, isThresholdKey, stopCondition, thresholdsOf, validDuration, validNumber, validThreshold } from '../../lib/bench-model';
import { isTruthy, setKey, unknownKeys } from '../../lib/section-model';
import { activeScenario, applyScenario, scenariosOf, type Scenario } from '../../lib/bench-scenarios';
import { BenchLauncher } from './BenchResults';
import { Plus, X } from 'lucide-react';
import { count } from 'luvo/data/plural';

export function BenchEditor() {
  const parsed = useStore(s => s.collectionParsed);
  const setSectionKv = useStore(s => s.setSectionKv);
  const bench = parsed?.bench ?? {};
  const [metric, setMetric] = useState('latency_ms.p99');
  const [expr, setExpr] = useState('');

  const put = (key: string, value: string) => setSectionKv('bench', setKey(bench, key, value));
  const thresholds = thresholdsOf(bench);

  const addThreshold = () => {
    const key = metric.trim() ? `thresholds.${metric.trim()}` : 'thresholds';
    if (!validThreshold(expr)) return;
    put(key, expr.trim());
    setExpr('');
  };

  const configured = Object.keys(bench).filter(k => !isThresholdKey(k)).length;
  const [opened, setOpened] = useState<string[]>([]);
  const [adding, setAdding] = useState(false);
  const addRef = useDismiss<HTMLDivElement>(adding, () => setAdding(false));
  const stop = stopCondition(bench);
  const toAdd = fieldsToAdd(bench, opened).map(group => ({
    ...group,
    fields: group.fields,
    applies: groupApplies(group, bench),
  }));

  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  useEffect(() => {
    let live = true;
    fetch('/api/bench/profiles')
      .then(r => (r.ok ? r.json() : []))
      .then(served => { if (live) setScenarios(scenariosOf(served)); })
      .catch(() => {});
    return () => { live = false; };
  }, []);
  const scenario = activeScenario(bench, scenarios);
  const [showScenarios, setShowScenarios] = useState(false);
  const scenariosOpen = configured === 0 || showScenarios;

  return (
    <div className="stack">
      {scenariosOpen && (
        <div className="scenarios">
          {scenarios.map(s => (
            <button
              key={s.name}
              className={`scenario${scenario === s.name ? ' is-on' : ''}`}
              onClick={() => setSectionKv('bench', applyScenario(bench, s, scenarios))}
              title={Object.entries(s.keys).map(([k, v]) => `${k}: ${v}`).join('\n')}
            >
              <span className="scenario-title">{s.name}</span>
              <span className="scenario-detail mono">{s.description}</span>
            </button>
          ))}
        </div>
      )}

      <BenchLauncher>
        <span className="label">
          {configured === 0 ? 'no BENCH section — a starting point above makes one' : count(configured, 'key')}
        </span>
        {configured > 0 && (
          <button className="btn is-ghost is-sm" onClick={() => setShowScenarios(v => !v)}>
            {showScenarios ? 'hide starting points' : 'starting points'}
          </button>
        )}
        {configured > 0 && (
          <button className="btn is-ghost is-sm" onClick={() => { setOpened([]); setSectionKv('bench', {}); }}>clear</button>
        )}
      </BenchLauncher>

      {BENCH_GROUPS.map(group => ({ group, fields: fieldsInUse(group, bench, opened) }))
        .filter(({ fields }) => fields.length > 0)
        .map(({ group, fields }) => (
        <fieldset key={group.title} className="panel">
          <legend>{group.title}</legend>
          <div className="panel-body">
            <div className="kvrow bench-row">
              {fields.map(field => {
                const value = bench[field.key] ?? '';
                const invalid =
                  (field.kind === 'duration' && !validDuration(value)) ||
                  (field.kind === 'number' && !validNumber(value));
                const overruled = stop.ignored === field.key;
                return (
                  <label
                    key={field.key}
                    className={`stack opt-field${overruled ? ' is-overruled' : ''}`}
                    title={overruled ? `duration is set, so this file stops at ${bench.duration} — requests is not read` : field.hint}
                  >
                    <span className="label">
                      {field.label}
                      {overruled && <span className="muted"> · not used</span>}
                    </span>
                    {field.kind === 'enum' ? (
                      <Seg
                        className="bench-seg"
                        label={field.key}
                        value={value}
                        onChange={v => put(field.key, value === v ? '' : v)}
                        options={field.values.map(v => ({ value: v, label: v }))}
                      />
                    ) : field.kind === 'bool' ? (
                      <label className="bar is-tight">
                        <input type="checkbox" checked={isTruthy(value)}
                          onChange={e => put(field.key, e.target.checked ? 'true' : '')} />
                        <span className="muted">{field.hint ?? 'off'}</span>
                      </label>
                    ) : (
                      <div className={`field-frame${invalid ? ' is-bad' : ''}`}>
                        <input className="field mono" placeholder={field.hint ?? ''} value={value}
                          onChange={e => put(field.key, e.target.value)} />
                      </div>
                    )}
                  </label>
                );
              })}
            </div>
          </div>
        </fieldset>
      ))}

      {toAdd.length > 0 && (
        <div className="picker" ref={addRef}>
          <button className="btn is-sm is-ghost" onClick={() => setAdding(v => !v)}>
            <Plus size={11} /> add a setting
          </button>
          <Popover open={adding} anchor={addRef} className="bench-add">
            <div className="menu">
              {toAdd.map(group => (
                <div key={group.title}>
                  <div className="menu-group">
                    {group.title}
                    {!group.applies && <span className="muted"> · other modes</span>}
                  </div>
                  {group.fields.map(field => (
                    <button
                      key={field.key}
                      className="menu-item"
                      title={field.hint}
                      onClick={() => { setOpened(prev => [...prev, field.key]); setAdding(false); }}
                    >
                      {field.label}
                    </button>
                  ))}
                </div>
              ))}
            </div>
          </Popover>
        </div>
      )}

      {unknownKeys(bench, [...benchKnownKeys(), ...thresholds.map(([k]) => k)]).length > 0 && (
        <div>
          <div className="label">also in this section</div>
          <div className="bar wrap">
            {unknownKeys(bench, [...benchKnownKeys(), ...thresholds.map(([k]) => k)]).map(([k, v]) => (
              <span key={k} className="chip mono">{k}: {v}</span>
            ))}
          </div>
          <div className="muted">Kept as written; edit them in the source tab.</div>
        </div>
      )}

      <fieldset className="panel">
        <legend>thresholds</legend>
        <div className="panel-body stack thresholds">
          {thresholds.length === 0 && (
            <div className="muted">No thresholds — the run reports numbers but cannot fail on them.</div>
          )}
          {thresholds.map(([key, value]) => (
            <div key={key} className="bar threshold-row">
              <span className="mono grow">{key.replace(/^thresholds\.?/, '') || 'overall'}</span>
              <span className="mono">{value}</span>
              <button className="btn is-ghost is-icon" aria-label={`Remove ${key}`} onClick={() => put(key, '')}>
                <X size={11} />
              </button>
            </div>
          ))}
          <div className="bar threshold-row">
            <div className="field-frame grow">
              <input className="field mono" placeholder="latency_ms.p99 · pass_rate_pct · rps"
                list="bench-metrics"
                value={metric} onChange={e => setMetric(e.target.value)} />
              <datalist id="bench-metrics">
                {BENCH_METRICS.map(m => <option key={m} value={m} />)}
              </datalist>
            </div>
            <div className={`field-frame is-narrow${expr && !validThreshold(expr) ? ' is-bad' : ''}`}>
              <input className="field mono" placeholder="< 250" value={expr}
                onChange={e => setExpr(e.target.value)}
                onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); addThreshold(); } }} />
            </div>
            <button className="btn is-ghost is-sm" onClick={addThreshold} disabled={!validThreshold(expr)}>
              <Plus size={11} /> add
            </button>
          </div>
          <div className="note">
            Thresholds live only in this section — there is no CLI flag for them. A failure exits 1
            and the report is still written, so the numbers survive the failure.
          </div>
        </div>
      </fieldset>
    </div>
  );
}
