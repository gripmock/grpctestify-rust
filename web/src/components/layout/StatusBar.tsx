import { useStore } from '../../lib/store';
import { colors } from '../../lib/theme';
import { FlaskConical, FolderGit2, Fingerprint, CircleHelp } from 'lucide-react';

export function StatusBar() {
  const totalOk = useStore(s => s.totalOk);
  const totalError = useStore(s => s.totalError);
  const version = useStore(s => s.version);
  const lastResponse = useStore(s => s.response);
  const projectRoot = useStore(s => s.projectRoot);
  const sessionId = useStore(s => s.sessionId);
  const address = useStore(s => s.address);

  
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

      <span>{totalOk + totalError} call{totalOk + totalError !== 1 ? 's' : ''}</span>
      {totalOk > 0 && <span style={{ color: colors.success }}>✓{totalOk}</span>}
      {totalError > 0 && <span style={{ color: colors.error }}>✗{totalError}</span>}

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

      <Divider />
      <button
        onClick={() => useStore.getState().setShowHotkeyHelp(true)}
        title="Keyboard shortcuts (?)"
        style={{
          background: 'none', border: 'none', cursor: 'pointer',
          color: 'var(--text-muted)', padding: 4,
          display: 'flex', alignItems: 'center',
          fontSize: 11, lineHeight: 1,
          borderRadius: 4,
        }}
        onMouseEnter={e => (e.currentTarget.style.color = 'var(--text-primary)')}
        onMouseLeave={e => (e.currentTarget.style.color = 'var(--text-muted)')}
      >
        <CircleHelp size={12} />
      </button>
    </footer>
  );
}

function Divider() {
  return <span style={{ opacity: 0.3 }}>|</span>;
}
