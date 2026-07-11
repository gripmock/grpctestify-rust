import { useStore } from '../../lib/store';
import { colors } from '../../lib/theme';
import { FlaskConical, FolderGit2, Fingerprint } from 'lucide-react';

export function StatusBar() {
  const history = useStore(s => s.history);
  const address = useStore(s => s.address);
  const version = useStore(s => s.version);
  const lastResponse = useStore(s => s.response);
  const projectRoot = useStore(s => s.projectRoot);
  const sessionId = useStore(s => s.sessionId);

  const ok = history.filter(h => h.response?.status === 'ok').length;
  const err = history.filter(h => h.response?.status === 'error').length;

  
  const connColor = lastResponse?.status === 'ok' ? colors.success
    : lastResponse?.status === 'error' ? colors.error
    : 'var(--text-muted)';

  return (
    <footer style={{
      height: 24, borderTop: '1px solid var(--border)',
      display: 'flex', alignItems: 'center', padding: '0 10px',
      fontSize: 11, color: 'var(--text-muted)', background: 'var(--bg-secondary)',
      gap: 10, flexShrink: 0,
    }}>
      <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
        <FlaskConical size={12} color={colors.accent} />
        {version ? `v${version}` : 'grpctestify'}
      </span>

      <Divider />

      <span>{history.length} call{history.length !== 1 ? 's' : ''}</span>
      {ok > 0 && <span style={{ color: colors.success }}>✓{ok}</span>}
      {err > 0 && <span style={{ color: colors.error }}>✗{err}</span>}

      <div style={{ flex: 1 }} />

      {projectRoot && (
        <span style={{ display: 'flex', alignItems: 'center', gap: 3, fontSize: 10, color: '#a855f7' }}>
          <FolderGit2 size={11} /> .grpctestify
          <Divider />
        </span>
      )}

      {sessionId && (
        <span style={{ display: 'flex', alignItems: 'center', gap: 3, fontSize: 10, color: 'var(--text-muted)' }}
          title="Your browser session ID — history files are tagged with this">
          <Fingerprint size={11} /> {sessionId}
          <Divider />
        </span>
      )}

      {address && (
        <span style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
          <span style={{ width: 7, height: 7, borderRadius: '50%', background: connColor, flexShrink: 0 }} />
          <span style={{ fontFamily: 'monospace', fontSize: 10 }}>{address}</span>
        </span>
      )}
    </footer>
  );
}

function Divider() {
  return <span style={{ opacity: 0.3 }}>|</span>;
}
