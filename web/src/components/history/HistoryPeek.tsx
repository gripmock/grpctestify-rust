import { useEffect, useRef } from 'react';
import type { HistoryEntry } from '../../lib/types';
import { copyToClipboard } from 'luvo/data/clipboard';
import { useToast } from 'luvo/ui/useToast';
import { entryFailed } from '../../lib/call-outcome';
import { looksHttp } from '../../lib/http-endpoint';
import { httpStatusLabel, httpStatusTone, durationLabel } from '../../lib/format';
import { Copy, ExternalLink, Play, X } from 'lucide-react';
import { explainFailure } from '../../lib/failure';
import { count } from 'luvo/data/plural';
import { maskHeader } from '../../lib/secret-headers';

const CAP = 20_000;

function pretty(text: string): string {
  try {
    return JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    return text;
  }
}

function body(value: unknown): string {
  const text = typeof value === 'string' ? pretty(value) : JSON.stringify(value, null, 2) ?? String(value);
  return text.length > CAP ? `${text.slice(0, CAP)}\n… ${text.length - CAP} more characters` : text;
}

export function HistoryPeek({ entry, top, panelRef, onClose, onOpen, onReplay }: {
  entry: HistoryEntry;
  top: number;
  panelRef: React.RefObject<HTMLDivElement | null>;
  onClose: () => void;
  onOpen: (entry: HistoryEntry) => void;
  onReplay: (entry: HistoryEntry) => void;
}) {
  const toast = useToast();
  const ref = useRef<HTMLDivElement>(null);
  const ok = !entryFailed(entry);
  const messages = entry.response.messages ?? [];
  const httpCode = looksHttp(entry.endpoint) ? entry.response.statusCode ?? null : null;

  useEffect(() => { ref.current?.focus(); }, [entry.id]);

  const copy = async (text: string, what: string) => {
    try {
      await copyToClipboard(text);
      toast.success(`${what} copied`);
    } catch {
      toast.error('The browser refused the clipboard');
    }
  };

  return (
    <div
      ref={node => { ref.current = node; panelRef.current = node; }}
      className="history-peek"
      role="dialog"
      aria-label={`${entry.endpoint} — what it sent and what came back`}
      tabIndex={-1}
      style={{ top: `${top}px` }}
      onKeyDown={e => {
        if (e.key === 'Escape') { e.preventDefault(); onClose(); return; }
        if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
          e.preventDefault();
          const rows = [...document.querySelectorAll<HTMLElement>('.history-row.is-call')];
          const at = rows.findIndex(r => r.classList.contains('is-peeked'));
          const next = rows[at + (e.key === 'ArrowDown' ? 1 : -1)];
          next?.click();
          next?.scrollIntoView({ block: 'nearest' });
        }
      }}
    >
      <div className="bar peek-head">
        <span className={`badge ${ok ? 'is-ok' : 'is-fail'}`}>{ok ? 'ok' : 'failed'}</span>
        <span className="mono grow peek-endpoint" title={entry.endpoint}>{entry.endpoint}</span>
        {httpCode !== null && (
          <span className={`mono history-status is-${httpStatusTone(httpCode) ?? 'fail'}`} title={httpStatusLabel(httpCode) ?? ''}>
            {httpCode}
          </span>
        )}
        {entry.checks && (
          <span
            className={`badge${entry.checks.passed === entry.checks.total ? ' is-ok' : ' is-fail'}`}
            title={`${entry.checks.passed} of ${entry.checks.total} checks passed`}
          >
            {entry.checks.passed}/{entry.checks.total}
          </span>
        )}
        {entry.response.durationMs != null && <span className="muted mono">{durationLabel(entry.response.durationMs)}</span>}
        <button className="btn is-ghost is-icon is-sm" onClick={onClose} aria-label="Close"><X size={12} /></button>
      </div>

      <div className="bar peek-conn muted mono">
        {entry.connection
          ? [entry.connection.address, entry.connection.protocol, entry.connection.tls ? 'tls' : '']
              .filter(Boolean).join(' · ')
          : 'recorded before the connection was kept'}
        {Object.keys(entry.headers ?? {}).length > 0 && (
          <span className="peek-headers" title={Object.entries(entry.headers).map(([k, v]) => `${k}: ${maskHeader(k, v)}`).join('\n')}>
            · {count(Object.keys(entry.headers).length, 'header')}
          </span>
        )}
      </div>

      <div className="peek-body">
        <div className="peek-half">
          <div className="bar">
            <span className="field-label grow">sent</span>
            {entry.resolved && entry.resolved.length > 0 && (
              <span
                className="muted peek-filled"
                title={`${entry.resolved.join(', ')} — filled in where the call was made, so the wire carried something else`}
              >
                {count(entry.resolved.length, 'name')} filled in
              </span>
            )}
            {entry.datasetRow !== undefined && (
              <span className="muted mono" title="The DATASET row this call was made with">
                row {entry.datasetRow + 1}
              </span>
            )}
            <button className="btn is-sm is-ghost" onClick={() => void copy(entry.bodies.join('\n'), 'Request')}>
              <Copy size={11} /> copy
            </button>
          </div>
          {entry.bodies.map((b, i) => (
            <pre key={i} className="peek-json mono">{body(b)}</pre>
          ))}
        </div>

        <div className="peek-half">
          <div className="bar">
            <span className="field-label grow">
              came back{messages.length > 1 ? ` · ${messages.length} messages` : ''}
            </span>
            {messages.length > 0 && (
              <button
                className="btn is-sm is-ghost"
                onClick={() => void copy(messages.map(m => body(m)).join('\n'), 'Response')}
              >
                <Copy size={11} /> copy
              </button>
            )}
          </div>
          {entry.response.error && (() => {
            const failure = explainFailure(
              entry.response.error,
              entry.response.statusCode ?? null,
              entry.connection?.address ?? null,
            );
            return (
              <div className="failure">
                <div className="assert is-fail">
                  <span className="assert-mark">!</span>
                  <span>{failure.title}</span>
                </div>
                {failure.detail && <div className="mono failure-detail">{failure.detail}</div>}
              </div>
            );
          })()}
          {messages.map((m, i) => (
            <pre key={i} className="peek-json mono">{body(m)}</pre>
          ))}
          {messages.length === 0 && !entry.response.error && (
            <div className="muted">The call returned no message.</div>
          )}
        </div>
      </div>

      <div className="bar peek-foot">
        <span className="grow" />
        <button className="btn is-sm is-ghost" onClick={() => onReplay(entry)}>
          <Play size={11} /> send again
        </button>
        <button className="btn is-sm" onClick={() => onOpen(entry)}>
          <ExternalLink size={11} /> open in a tab
        </button>
      </div>
    </div>
  );
}
