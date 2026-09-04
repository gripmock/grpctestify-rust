import { useStore } from '../../lib/store';
import { Seg } from 'luvo/ui/Seg';
import { isHttpRequest } from '../../lib/http-endpoint';
import { COMPRESSION, PROTOCOLS, isTruthy, numberValue, setKey, unknownKeys } from '../../lib/section-model';
import { keyProblem } from '../../lib/assert-problems';
import { delayUnused, overruledBy } from '../../lib/options-override';

const KNOWN = ['timeout', 'retry', 'retry_delay', 'no_retry', 'no-retry', 'compression', 'protocol'];

export function OptionsEditor() {
  const isHttp = useStore(s => isHttpRequest(s.workspacePath, s.request.endpoint));
  const parsed = useStore(s => s.collectionParsed);
  const setSectionKv = useStore(s => s.setSectionKv);
  const diagnostics = useStore(s => s.diagnostics);
  const options = parsed?.options ?? {};
  const attributes = parsed?.attributes ?? [];

  const put = (key: string, value: string) => setSectionKv('options', setKey(options, key, value));

  const num = (key: string, opts: { min?: number; integer?: boolean }) => (raw: string) => {
    const value = numberValue(raw, opts);
    if (value !== null) put(key, value);
  };

  const retryConflict = isTruthy(options.no_retry) && Number(options.retry ?? '0') > 0;

  const has = (key: string) =>
    (options[key] ?? options[key.replace('_', '-')] ?? '').trim() !== '';
  const overruled = (key: string) => (has(key) ? overruledBy(attributes, key) : null);
  const written = (key: string, by: { section: string; value: string }) =>
    `${by.section} #[${key}${by.value !== 'true' ? `(${by.value})` : ''}]`;
  const beaten = (key: string) =>
    `${written(key, overruled(key)!)} is what the run reads — this line is not`;

  return (
    <div className="stack">
      <div className="kvrow opt-row">
        <label className={`stack opt-field${overruled('timeout') ? ' is-overruled' : ''}`}
          title={overruled('timeout') ? beaten('timeout') : undefined}>
          <span className="field-label">timeout{overruled('timeout') && <span className="muted"> · not used</span>}</span>
          <div className="field-frame">
            <input className="field mono" inputMode="numeric" placeholder="seconds"
              value={options.timeout ?? ''} onChange={e => num('timeout', { integer: true, min: 1 })(e.target.value)} />
            <span className="expr-gutter">s</span>
          </div>
        </label>
        <label className={`stack opt-field${overruled('retry') ? ' is-overruled' : ''}`}
          title={overruled('retry') ? beaten('retry') : undefined}>
          <span className="field-label">retry{overruled('retry') && <span className="muted"> · not used</span>}</span>
          <div className="field-frame">
            <input className="field mono" inputMode="numeric" placeholder="attempts"
              value={options.retry ?? ''} onChange={e => num('retry', { integer: true, min: 0 })(e.target.value)} />
          </div>
        </label>
        <label className={`stack opt-field${overruled('retry_delay') ? ' is-overruled' : ''}`}
          title={overruled('retry_delay') ? beaten('retry_delay') : undefined}>
          <span className="field-label">retry delay{overruled('retry_delay') && <span className="muted"> · not used</span>}</span>
          <div className="field-frame">
            <input className="field mono" inputMode="decimal" placeholder="seconds"
              value={options.retry_delay ?? ''} onChange={e => num('retry_delay', { min: 0 })(e.target.value)} />
            <span className="expr-gutter">s</span>
          </div>
        </label>
      </div>

      <div className="bar wrap">
        {!isHttp && (
          <>
        <span className="field-label">compression</span>
        <Seg
          label="Compression"
          value={options.compression ?? 'none'}
          onChange={v => put('compression', v === 'none' ? '' : v)}
          options={COMPRESSION.map(v => ({ value: v, label: v }))}
        />

        <span className="field-label">protocol</span>
        <Seg
          label="Protocol"
          value={options.protocol ?? 'grpc'}
          onChange={v => put('protocol', v === 'grpc' ? '' : v)}
          options={PROTOCOLS.map(v => ({ value: v, label: v }))}
        />
          </>
        )}

        <label className="bar is-tight">
          <input type="checkbox" checked={isTruthy(options.no_retry)}
            onChange={e => put('no_retry', e.target.checked ? 'true' : '')} />
          <span>no retry</span>
        </label>
        {overruled('no_retry') && (
          <span className="muted">{beaten('no_retry')}</span>
        )}
        {!isHttp && overruled('compression') && (
          <span className="muted">{beaten('compression')}</span>
        )}
      </div>

      {retryConflict && (
        <div className="note">
          <span className="mono">no_retry</span> with <span className="mono">retry: {options.retry}</span> —
          the runner honours <span className="mono">no_retry</span> and the retry count is dead config.
        </div>
      )}

      {Number(options.retry ?? '0') > 0 && (
        <div className="note">
          A <span className="mono">retry</span> is what a run does with a failing test — Execute makes
          one call and shows you what came back.
        </div>
      )}

      {delayUnused(options, attributes) && (
        <div className="note">
          A <span className="mono">retry_delay</span> is the wait between attempts — with no retries
          it is config nothing reads.
        </div>
      )}

      {options.timeout !== undefined && !overruled('timeout') && (
        <div className="note">
          A <span className="mono">timeout</span> here outranks the wait set beside the address and
          <span className="mono"> run --timeout</span>: CI waits what this file says. A section's own
          <span className="mono"> #[timeout]</span> outranks both.
        </div>
      )}

      {!isHttp && (
        <div className="note">
          The transport in the top bar is written here as <span className="mono">OPTIONS.protocol</span>,
          so CI runs what you tested against.
        </div>
      )}

      {unknownKeys(options, KNOWN).length > 0 && (
        <div>
          <div className="field-label">also in this section</div>
          <div className="bar wrap">
            {unknownKeys(options, KNOWN).map(([k, v]) => (
              <span key={k} className="chip mono is-warn" title={keyProblem(k, diagnostics) ?? undefined}>
                {k}: {v}
              </span>
            ))}
          </div>
          <div className="muted">
            The runner reads none of these — they are kept in the file, and edited in the source tab.
          </div>
          {unknownKeys(options, KNOWN)
            .map(([k]) => keyProblem(k, diagnostics))
            .filter((said): said is string => said !== null)
            .map((said, i) => <div key={i} className="note is-warn">{said}</div>)}
        </div>
      )}

      {attributes.length > 0 && (
        <div>
          <div className="field-label">section attributes</div>
          <div className="bar wrap">
            {attributes.map(a => (
              <span key={`${a.section}${a.index}${a.name}=${a.value}`} className="chip mono">
                {a.section} #[{a.name}{a.value !== 'true' ? `(${a.value})` : ''}]
              </span>
            ))}
          </div>
          <div className="muted">Read from the file; edit them in the source tab.</div>
        </div>
      )}
    </div>
  );
}
