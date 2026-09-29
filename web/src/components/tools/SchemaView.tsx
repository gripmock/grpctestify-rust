import { useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../../lib/store';
import { schemaRequest } from '../../lib/schema-request';
import { filterProto } from '../../lib/proto-filter';
import { schemaMiss, servicesOf } from '../../lib/schema-miss';
import { callAddress } from '../../lib/store';
import { copyToClipboard } from 'luvo/data/clipboard';
import { useToast } from 'luvo/ui/useToast';
import { Copy, Loader2, RefreshCw } from 'lucide-react';

const IDLE = { kind: 'idle' } as const;

type State =
  | { kind: 'idle' }
  | { kind: 'loading' }
  | { kind: 'ok'; source: string }
  | { kind: 'error'; reason: string };

export function SchemaView() {
  const endpoint = useStore(s => s.request.endpoint);
  const address = useStore(s => s.address);
  const protocol = useStore(s => s.protocol);
  const collection = useStore(s => s.selectedCollection);
  const target = useStore(callAddress);
  const reflected = useStore(s => s.reflectionMethods);
  const toast = useToast();

  const [state, setState] = useState<State>({ kind: 'idle' });
  const [filter, setFilter] = useState('');
  const [nonce, setNonce] = useState(0);
  const abort = useRef<AbortController | null>(null);

  useEffect(() => {
    abort.current?.abort();
    if (!endpoint) return;
    const controller = new AbortController();
    abort.current = controller;

    const timer = setTimeout(async () => {
      setState({ kind: 'loading' });
      try {
        const res = await fetch('/api/proto-source', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(schemaRequest(useStore.getState(), endpoint)),
          signal: controller.signal,
        });
        const data = await res.json();
        if (controller.signal.aborted) return;
        if (data.error || !data.source) {
          setState({ kind: 'error', reason: data.error || 'The target returned no schema' });
          return;
        }
        setState({ kind: 'ok', source: data.source });
      } catch (err: any) {
        if (controller.signal.aborted) return;
        setState({ kind: 'error', reason: err?.message || String(err) });
      }
    }, 250);

    return () => { clearTimeout(timer); controller.abort(); };
  }, [endpoint, address, protocol, collection, nonce]);

  const view: State = endpoint ? state : IDLE;

  const shown = useMemo(
    () => (view.kind === 'ok' ? filterProto(view.source, filter) : ''),
    [view, filter],
  );

  return (
    <div className="stack schema-view">
      <div className="bar">
        <input
          className="field field-frame mono grow"
          placeholder="filter fields"
          value={filter}
          spellCheck={false}
          onChange={e => setFilter(e.target.value)}
          disabled={state.kind !== 'ok'}
        />
        <button
          className="btn is-sm is-ghost is-icon"
          onClick={() => setNonce(n => n + 1)}
          disabled={!endpoint || view.kind === 'loading'}
          title="Read the schema again"
          aria-label="Read the schema again"
        >
          {view.kind === 'loading' ? <Loader2 size={12} className="animate-spin" /> : <RefreshCw size={12} />}
        </button>
        <button
          className="btn is-sm is-ghost"
          disabled={view.kind !== 'ok'}
          onClick={async () => {
            if (view.kind !== 'ok') return;
            try {
              await copyToClipboard(view.source);
              toast.success('Schema copied');
            } catch {
              toast.error('The browser refused the clipboard');
            }
          }}
        >
          <Copy size={11} /> copy
        </button>
      </div>

      {view.kind === 'idle' && <div className="empty-state">Choose a method — its definition is read from the target or from the file’s PROTO section</div>}
      {view.kind === 'loading' && <div className="empty-state">Reading the schema…</div>}
      {view.kind === 'error' && (() => {
        const miss = schemaMiss({ reason: view.reason, address: target, services: servicesOf(reflected) });
        if (!miss) {
          return <div className="assert is-fail"><span className="assert-mark">!</span><span>{view.reason}</span></div>;
        }
        return (
          <div className="assert is-fail">
            <span className="assert-mark">!</span>
            <span className="stack is-tight">
              <span>{miss.title}</span>
              {miss.services.length > 0
                ? (
                  <span className="muted">
                    That target serves{' '}
                    {miss.services.map((name, i) => (
                      <span key={name}>
                        {i > 0 ? ' · ' : ''}<span className="mono">{name}</span>
                      </span>
                    ))}
                  </span>
                )
                : (
                  <span className="muted">
                    Nothing has asked it what it serves yet — the endpoint field asks, or the file’s
                    PROTO section can name a descriptor.
                  </span>
                )}
            </span>
          </div>
        );
      })()}
      {view.kind === 'ok' && (
        <pre className="proto-source mono">{shown !== '' ? shown : `nothing matches “${filter}”`}</pre>
      )}
    </div>
  );
}
