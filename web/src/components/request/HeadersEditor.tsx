import { useStore } from '../../lib/store';
import { colors } from '../../lib/theme';
import { Plus, X } from 'lucide-react';
import { EnvVarToolbar } from './EnvVarToolbar';

function resolveValue(val: string, env: { variables: Record<string, string> } | null): string {
  if (!env) return val;
  let r = val;
  for (const [k, v] of Object.entries(env.variables)) {
    r = r.replaceAll(`{{${k}}}`, v);
  }
  return r;
}

export function HeadersEditor() {
  const request = useStore(s => s.request);
  const setRequestHeaders = useStore(s => s.setRequestHeaders);
  const activeEnv = useStore(s => {
    const ae = s.activeEnvironment;
    return ae ? s.environments.find(e => e.name === ae) : null;
  });

  const entries = Object.entries(request.headers);

  const set = (key: string, value: string, oldKey?: string) => {
    const h = { ...request.headers };
    if (oldKey !== undefined && oldKey !== key) delete h[oldKey];
    if (key) h[key] = value;
    else delete h[oldKey!];
    setRequestHeaders(h);
  };

  const add = () => {
    setRequestHeaders({ ...request.headers, '': '' });
  };

  const allValues = entries.map(([, v]) => v).join('\n');

  return (
    <div>
      <div style={{ border: '1px solid var(--border)', borderRadius: 6, padding: 8 }}>
        {entries.length === 0 && (
          <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 8 }}>No headers</div>
        )}
        {entries.map(([k, v], i) => {
          const hasVarPattern = v.includes('{{');
          const resolved = hasVarPattern && activeEnv ? resolveValue(v, activeEnv) : v;
          const isDifferent = resolved !== v;
          return (
          <div key={i} style={{ display: 'flex', gap: 6, marginBottom: 4, alignItems: 'center' }}>
            <input
              value={k}
              onChange={e => set(e.target.value, v, k)}
              placeholder="Key"
              style={{ flex: 1, padding: '4px 8px', fontSize: 12, borderRadius: 4, border: '1px solid var(--border)', background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none', fontFamily: 'monospace' }}
            />
            <div style={{ position: 'relative', flex: 2 }}>
              <input
                value={v}
                onChange={e => set(k, e.target.value, k)}
                placeholder="Value"
                title={isDifferent ? `Resolves to: ${resolved}` : undefined}
                style={{
                  width: '100%', padding: '4px 8px', fontSize: 12, borderRadius: 4, boxSizing: 'border-box',
                  border: hasVarPattern && activeEnv ? `2px solid ${colors.accent}` : '1px solid var(--border)',
                  background: hasVarPattern && activeEnv ? `${colors.accent}08` : 'var(--bg-primary)',
                  color: 'var(--text-primary)', outline: 'none', fontFamily: 'monospace',
                }}
              />
            </div>
            {isDifferent && (
              <span style={{
                fontSize: 9, padding: '2px 5px', borderRadius: 3, whiteSpace: 'nowrap',
                background: `${colors.accent}15`, color: colors.accent,
                fontFamily: 'monospace', flexShrink: 0,
              }} title={resolved}>
                {resolved}
              </span>
            )}
            <button onClick={() => set('', '', k)} style={{ background: 'none', border: 'none', cursor: 'pointer', padding: 4, color: 'var(--text-muted)', flexShrink: 0 }}>
              <X size={14} />
            </button>
          </div>
          );
        })}
        <button onClick={add} style={{ display: 'flex', alignItems: 'center', gap: 4, padding: '4px 8px', background: 'none', border: '1px dashed var(--border)', borderRadius: 4, cursor: 'pointer', fontSize: 12, color: 'var(--text-muted)', marginTop: 4 }}>
          <Plus size={12} /> Add header
        </button>
      </div>
      <EnvVarToolbar text={allValues} />
    </div>
  );
}
