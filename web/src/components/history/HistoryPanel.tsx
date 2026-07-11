import { useState, useMemo } from 'react';
import { useStore } from '../../lib/store';
import { btn, colors } from '../../lib/theme';
import { Trash2, Clock, Search, Play, Fingerprint } from 'lucide-react';

type SessionFilter = 'all' | 'mine' | string;

export function HistoryPanel() {
  const history = useStore(s => s.history);
  const sessionId = useStore(s => s.sessionId);
  const restoreHistory = useStore(s => s.restoreHistory);
  const clearHistory = useStore(s => s.clearHistory);
  const [search, setSearch] = useState('');
  const [sessionFilter, setSessionFilter] = useState<SessionFilter>('all');

  
  const sessions = useMemo(() => {
    const set = new Set<string>();
    for (const h of history) {
      const sid = (h as any)._session;
      if (sid) set.add(sid);
    }
    return [...set].sort();
  }, [history]);

  
  const filtered = useMemo(() => {
    let items = history;
    if (sessionFilter === 'mine') {
      items = items.filter(h => (h as any)._session === sessionId || !(h as any)._session);
    } else if (sessionFilter !== 'all') {
      items = items.filter(h => (h as any)._session === sessionFilter);
    }
    if (search) {
      items = items.filter(h => h.endpoint.toLowerCase().includes(search.toLowerCase()));
    }
    return items;
  }, [history, sessionFilter, sessionId, search]);

  const fmt = (ts: number) => {
    const d = new Date(ts);
    return `${d.getHours().toString().padStart(2, '0')}:${d.getMinutes().toString().padStart(2, '0')}:${d.getSeconds().toString().padStart(2, '0')}`;
  };

  return (
    <div style={{ padding: 8 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
        <span style={{ fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.6px' }}>History</span>
        {history.length > 0 && (
          <button onClick={clearHistory} style={btn('ghost', 'sm')} title="Clear"><Trash2 size={12} /></button>
        )}
      </div>

      {}
      {history.length > 0 && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 6, border: '1px solid var(--border)', borderRadius: 5, padding: '3px 6px', background: 'var(--bg-primary)' }}>
          <Search size={12} style={{ color: 'var(--text-muted)', flexShrink: 0 }} />
          <input value={search} onChange={e => setSearch(e.target.value)} placeholder="Filter by endpoint…"
            style={{ flex: 1, border: 'none', background: 'transparent', fontSize: 11, color: 'var(--text-primary)', outline: 'none' }} />
          {search && <button onClick={() => setSearch('')} style={{ ...btn('ghost', 'sm'), fontSize: 10, padding: '0 3px' }}>✕</button>}
        </div>
      )}

      {}
      {sessions.length > 0 && (
        <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 6, fontSize: 10, flexWrap: 'wrap' }}>
          <button onClick={() => setSessionFilter('all')} style={{
            ...btn('ghost', 'sm'), fontSize: 10,
            color: sessionFilter === 'all' ? colors.accent : 'var(--text-muted)',
          }}>All</button>
          <button onClick={() => setSessionFilter('mine')} style={{
            ...btn('ghost', 'sm'), fontSize: 10,
            color: sessionFilter === 'mine' ? colors.accent : 'var(--text-muted)',
          }}>Mine ({sessionId})</button>
          {sessions.filter(s => s !== sessionId).map(sid => (
            <button key={sid} onClick={() => setSessionFilter(sid)} style={{
              ...btn('ghost', 'sm'), fontSize: 10,
              color: sessionFilter === sid ? colors.accent : 'var(--text-muted)',
            }}>
              {sid}
            </button>
          ))}
        </div>
      )}

      {filtered.length === 0 && (
        <div style={{ padding: '12px 0', fontSize: 12, color: 'var(--text-muted)', textAlign: 'center' }}>
          {search || sessionFilter !== 'all' ? 'No matches' : 'No calls yet'}
        </div>
      )}

      {filtered.map(entry => {
        const entrySession = (entry as any)._session;
        const isMine = !entrySession || entrySession === sessionId;
        return (
          <div key={entry.id} onClick={() => restoreHistory(entry)} style={{
            padding: '5px 8px', borderRadius: 4, cursor: 'pointer', marginBottom: 2,
            borderLeft: `3px solid ${entry.response.status === 'ok' ? 'var(--success)' : 'var(--error)'}`,
            opacity: isMine ? 1 : 0.6,
          }}
            onMouseEnter={e => { (e.currentTarget as HTMLElement).style.background = 'var(--bg-tertiary)'; }}
            onMouseLeave={e => { (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
          >
            <div style={{ fontSize: 10, color: 'var(--text-muted)', display: 'flex', alignItems: 'center', gap: 3 }}>
              <Clock size={10} /> {fmt(entry.timestamp)}
              {entrySession && (
                <span style={{ display: 'flex', alignItems: 'center', gap: 2, fontSize: 9, color: isMine ? colors.accent : 'var(--text-muted)' }}>
                  <Fingerprint size={9} /> {entrySession}
                </span>
              )}
              <span style={{ flex: 1 }} />
              {entry.response.durationMs != null && <span>{entry.response.durationMs}ms</span>}
            </div>
            <div style={{ fontSize: 12, fontFamily: 'monospace', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', marginTop: 1 }}>
              {entry.endpoint.split('/').pop() || entry.endpoint}
            </div>
            <div style={{ fontSize: 11, display: 'flex', alignItems: 'center', gap: 4, marginTop: 1 }}>
              <span style={{ color: entry.response.status === 'ok' ? 'var(--success)' : 'var(--error)' }}>
                {entry.response.status === 'ok' ? '✓' : '✗'}
              </span>
              <span style={{ color: 'var(--text-muted)', fontSize: 10, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>
                {entry.endpoint}
              </span>
              <Play size={10} style={{ color: 'var(--text-muted)', flexShrink: 0 }} />
            </div>
          </div>
        );
      })}
    </div>
  );
}
