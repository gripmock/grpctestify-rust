import { useStore } from '../../lib/store';
import { Plus, X } from 'lucide-react';
import { EnvVarToolbar } from './EnvVarToolbar';

export function EnvEditor() {
  const store = useStore();
  const environment = store.environment;
  const setEnvironment = store.setEnvironment;
  const entries = Object.entries(environment || {});

  const set = (key: string, value: string, oldKey?: string) => {
    const h = { ...environment };
    if (oldKey !== undefined && oldKey !== key) delete h[oldKey];
    if (key) h[key] = value;
    else delete h[oldKey!];
    setEnvironment(h);
  };

  const add = () => setEnvironment({ ...environment, '': '' });

  return (
    <div style={{ border: '1px solid var(--border)', borderRadius: 6, padding: 8 }}>
      <div style={{ fontSize: 11, color: 'var(--text-muted)', marginBottom: 8 }}>
        Use <code style={{ background: 'var(--bg-tertiary)', padding: '1px 4px', borderRadius: 3, fontSize: 11 }}>{'{{KEY}}'}</code> in requests to substitute environment variables.
      </div>
      {entries.length === 0 && <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 8 }}>No variables</div>}
      {entries.map(([k, v], i) => (
        <div key={i} style={{ display: 'flex', gap: 6, marginBottom: 4 }}>
          <input value={k} onChange={e => set(e.target.value, v, k)} placeholder="$KEY" style={{ flex: 1, padding: '4px 8px', fontSize: 12, borderRadius: 4, border: '1px solid var(--border)', background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none', fontFamily: 'monospace' }} />
          <input value={v} onChange={e => set(k, e.target.value, k)} placeholder="Value" style={{ flex: 2, padding: '4px 8px', fontSize: 12, borderRadius: 4, border: '1px solid var(--border)', background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none', fontFamily: 'monospace' }} />
          <button onClick={() => set('', '', k)} style={{ background: 'none', border: 'none', cursor: 'pointer', padding: 4, color: 'var(--text-muted)' }}><X size={14} /></button>
        </div>
      ))}
      <button onClick={add} style={{ display: 'flex', alignItems: 'center', gap: 4, padding: '4px 8px', background: 'none', border: '1px dashed var(--border)', borderRadius: 4, cursor: 'pointer', fontSize: 12, color: 'var(--text-muted)', marginTop: 4 }}>
        <Plus size={12} /> Add variable
      </button>
      <EnvVarToolbar text={Object.keys(environment).join('\n')} />
    </div>
  );
}
