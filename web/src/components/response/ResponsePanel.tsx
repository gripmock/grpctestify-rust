import { MonacoEditor as Editor } from '../MonacoEditor';
import { useState } from 'react';
import { useStore } from '../../lib/store';
import { colors } from '../../lib/theme';
import { Loader2, Check, X, Layers, Clock, ArrowRight } from 'lucide-react';

function msgPreview(msg: unknown, maxLen = 60): string {
  const s = JSON.stringify(msg);
  if (!s || s === 'null') return '(null)';
  return s.length > maxLen ? s.slice(0, maxLen) + '…' : s;
}

export function ResponsePanel() {
  const response = useStore(s => s.response);
  const setResponseTab = useStore(s => s.setResponseTab);
  const responseTab = useStore(s => s.responseTab);
  const [selectedMsg, setSelectedMsg] = useState(0);

  const monacoTheme = useStore(s => s.theme === 'dark' ? 'vs-dark' : 'light');

  const msgs: unknown[] = response?.messages ?? [];
  const msgCount = msgs.length;
  const isStreaming = msgCount > 1;

  
  const totalMs = response?.durationMs ?? 0;
  const perMsgMs = msgCount > 0 ? totalMs / msgCount : 0;

  return (
    <section>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 6, minHeight: 22 }}>
        <span style={{ fontWeight: 600, fontSize: 13 }}>Response</span>

        {response?.status === 'pending' && (
          <span style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 12, color: colors.warning }}>
            <Loader2 size={14} className="animate-spin" /> executing…
          </span>
        )}
        {response?.status === 'ok' && (
          <span style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 12, color: colors.success }}>
            <Check size={14} /> {msgCount} msg{msgCount !== 1 ? 's' : ''}
          </span>
        )}
        {response?.status === 'error' && (
          <span style={{ display: 'flex', alignItems: 'center', gap: 4, fontSize: 12, color: colors.error }}>
            <X size={14} /> Error
          </span>
        )}
        {response?.durationMs != null && (
          <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>{response.durationMs}ms</span>
        )}

        {!!isStreaming && (
          <span style={{ fontSize: 10, padding: '1px 6px', borderRadius: 4, background: `${colors.success}18`, color: colors.success, display: 'flex', alignItems: 'center', gap: 3 }}>
            <Layers size={10} /> streaming
          </span>
        )}
      </div>

      {!response && (
        <div style={{ padding: '32px 0', textAlign: 'center', fontSize: 13, color: 'var(--text-muted)' }}>
          Execute a call to see the response
        </div>
      )}

      {response && response.status !== 'pending' && (
        <>
          <div style={{ display: 'flex', borderBottom: '1px solid var(--border)', marginBottom: 8 }}>
            {(['response', 'headers'] as const).map(tab => (
              <button key={tab} onClick={() => setResponseTab(tab)} style={{
                padding: '5px 12px', fontSize: 11, cursor: 'pointer', border: 'none', background: 'none',
                transition: 'color 0.15s',
                color: responseTab === tab ? 'var(--accent)' : 'var(--text-secondary)',
                fontWeight: responseTab === tab ? 600 : 400,
                borderBottom: responseTab === tab ? '2px solid var(--accent)' : '2px solid transparent',
              }}>
                {tab === 'response' ? `Response${isStreaming ? ` (${msgCount})` : ''}` : 'Headers'}
              </button>
            ))}
          </div>

          {responseTab === 'response' && (
            <div>
              {msgCount === 0 && (
                <div style={{ padding: 12, fontSize: 12, color: 'var(--text-muted)' }}>
                  {response.error ? response.error : 'No response messages'}
                </div>
              )}

              {isStreaming && msgCount > 0 && (
                <div style={{ marginBottom: 12 }}>
                  <div style={{ display: 'flex', gap: 6, marginBottom: 6, fontSize: 10, color: 'var(--text-muted)', fontWeight: 600, textTransform: 'uppercase', letterSpacing: '0.5px' }}>
                    <span style={{ width: 32 }}>#</span>
                    <span style={{ width: 60 }}>Time</span>
                    <span>Message</span>
                  </div>

                  <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                    {msgs.map((msg, i) => (
                      <div key={i} onClick={() => setSelectedMsg(i)} style={{
                        display: 'flex', alignItems: 'center', gap: 6, padding: '5px 8px',
                        borderRadius: 5, cursor: 'pointer', fontSize: 12,
                        background: selectedMsg === i ? `${colors.accent}12` : 'transparent',
                        border: selectedMsg === i ? `1px solid ${colors.accent}30` : '1px solid transparent',
                        transition: 'all 0.1s ease',
                      }}
                        onMouseEnter={e => { if (selectedMsg !== i) e.currentTarget.style.background = 'var(--bg-tertiary)'; }}
                        onMouseLeave={e => { if (selectedMsg !== i) e.currentTarget.style.background = 'transparent'; }}
                      >
                        <span style={{ width: 32, color: 'var(--text-muted)', fontFamily: 'monospace', fontSize: 11 }}>#{i + 1}</span>
                        <span style={{ width: 60, display: 'flex', alignItems: 'center', gap: 2, color: 'var(--text-muted)', fontSize: 11, fontFamily: 'monospace' }}>
                          <Clock size={10} /> {Math.round(perMsgMs * (i + 1))}ms
                        </span>
                        <span style={{ flex: 1, fontFamily: 'monospace', fontSize: 11, color: 'var(--text-primary)', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                          {msgPreview(msg)}
                        </span>
                        <ArrowRight size={12} style={{ color: 'var(--text-muted)', flexShrink: 0 }} />
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {msgCount > 0 && (
                <div style={{ border: '1px solid var(--border)', borderRadius: 6, overflow: 'hidden' }}>
                  <Editor
                    height="250px"
                    language="json"
                    value={JSON.stringify(msgs[selectedMsg] ?? {}, null, 2)}
                    theme={monacoTheme}
                    options={{
                      readOnly: true, minimap: { enabled: false }, fontSize: 13,
                      scrollBeyondLastLine: false, wordWrap: 'on', automaticLayout: true,
                      lineNumbers: 'on', folding: true,
                    }}
                  />
                </div>
              )}
            </div>
          )}

          {responseTab === 'headers' && (
            <div style={{ border: '1px solid var(--border)', borderRadius: 6, padding: 8, fontSize: 12.5, fontFamily: 'monospace' }}>
              <div style={{ marginBottom: 8 }}>
                <span style={{ fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase' }}>Headers</span>
                {Object.keys(response.headers).length === 0 && <div style={{ color: 'var(--text-muted)', marginTop: 4 }}>None</div>}
                {Object.entries(response.headers).map(([k, v]) => (
                  <div key={k} style={{ padding: '2px 0' }}><span style={{ color: 'var(--accent)' }}>{k}</span>: {v}</div>
                ))}
              </div>
              {Object.keys(response.trailers || {}).length > 0 && (
                <>
                  <div style={{ height: 1, background: 'var(--border)', margin: '6px 0' }} />
                  <span style={{ fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase' }}>Trailers</span>
                  {Object.entries(response.trailers || {}).map(([k, v]) => (
                    <div key={k} style={{ padding: '2px 0' }}><span style={{ color: 'var(--accent)' }}>{k}</span>: {v}</div>
                  ))}
                </>
              )}
              {response.durationMs != null && (
                <>
                  <div style={{ height: 1, background: 'var(--border)', margin: '6px 0' }} />
                  <div><span style={{ color: 'var(--text-muted)' }}>Duration</span>: {response.durationMs}ms</div>
                </>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
}
