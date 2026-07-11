import { useMemo } from 'react';
import { useStore } from '../../lib/store';
import { colors } from '../../lib/theme';
import { Info } from 'lucide-react';


export function EnvVarToolbar({ text }: { text: string }) {
  const activeEnvironment = useStore(s => s.activeEnvironment);
  const environments = useStore(s => s.environments);

  const activeEnv = useMemo(
    () => environments.find(e => e.name === activeEnvironment),
    [environments, activeEnvironment],
  );

  if (!activeEnv) return null;

  
  const usedVars = useMemo(() => {
    if (!text) return [];
    const matches = text.match(/\{\{(\w+)\}\}/g);
    if (!matches) return [];
    const names = [...new Set(matches.map(m => m.slice(2, -2)))];
    return names
      .filter(k => k in activeEnv.variables)
      .map(k => ({ key: k, value: activeEnv.variables[k], muted: (activeEnv.mutedVariables || []).includes(k) }));
  }, [text, activeEnv]);

  if (usedVars.length === 0) return null;

  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 4, padding: '3px 8px',
      fontSize: 10, borderRadius: 4, marginTop: 4,
      background: `${colors.accent}08`, border: `1px solid ${colors.accent}18`,
      flexWrap: 'wrap',
    }}>
      <Info size={10} style={{ color: colors.accent, flexShrink: 0 }} />
      <span style={{ color: 'var(--text-muted)', fontWeight: 600 }}>{activeEnv.name}:</span>
      {usedVars.map(({ key, value, muted }) => (
        <span key={key} style={{
          padding: '1px 5px', borderRadius: 3, fontFamily: 'monospace', fontSize: 9,
          background: muted ? `${colors.warning}18` : 'var(--bg-primary)',
          color: muted ? colors.warning : 'var(--text-primary)',
          textDecoration: muted ? 'line-through' : 'none',
        }} title={muted ? `"${key}" is muted — will NOT be substituted` : value || 'empty (secret)'}>
          {key}={value || '••••••'}
        </span>
      ))}
    </div>
  );
}
