import { useState, useRef, useEffect, useMemo } from 'react';
import { useStore } from '../../lib/store';
import { btn, colors, css } from '../../lib/theme';
import { FlaskConical, Sun, Moon, Shield, ShieldOff, RefreshCw, Loader2, Check, X, Settings, FolderGit2, ChevronDown } from 'lucide-react';
import { EnvironmentManager } from '../request/EnvironmentManager';

export function TopBar() {
  const address = useStore(s => s.address);
  const setAddress = useStore(s => s.setAddress);
  const protocol = useStore(s => s.protocol);
  const setProtocol = useStore(s => s.setProtocol);
  const tls = useStore(s => s.tls);
  const setTls = useStore(s => s.setTls);
  const tlsInsecure = useStore(s => s.tlsInsecure);
  const setTlsInsecure = useStore(s => s.setTlsInsecure);
  const theme = useStore(s => s.theme);
  const setTheme = useStore(s => s.setTheme);
  const serverHealthy = useStore(s => s.serverHealthy);
  const reflect = useStore(s => s.reflect);
  const reflectStatus = useStore(s => s.reflectStatus);
  const reflectError = useStore(s => s.reflectError);
  const environments = useStore(s => s.environments);
  const activeEnvironment = useStore(s => s.activeEnvironment);
  const setActiveEnvironment = useStore(s => s.setActiveEnvironment);

  const projectRoot = useStore(s => s.projectRoot);
  const [showEnvManager, setShowEnvManager] = useState(false);
  const [showDropdown, setShowDropdown] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);

  const activeEnv = environments.find(e => e.name === activeEnvironment);

  
  const resolvedAddress = useMemo(() => {
    if (!activeEnv?.variables) return address;
    let r = address;
    for (const [k, v] of Object.entries(activeEnv.variables)) {
      r = r.replaceAll(`{{${k}}}`, v);
    }
    return r;
  }, [address, activeEnv]);
  const hasVarPattern = address?.includes('{{') ?? false;

  
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setShowDropdown(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  return (
    <header style={{
      height: 44, borderBottom: '1px solid var(--border)',
      display: 'flex', alignItems: 'center', padding: '0 10px', gap: 6,
      background: 'var(--bg-secondary)', flexShrink: 0,
    }}>
      {}
      <div style={{ display: 'flex', alignItems: 'center', gap: 5, fontWeight: 600, fontSize: 14, minWidth: 0 }}>
        <FlaskConical size={18} color={colors.accent} />
        <span>grpctestify</span>
        <span style={{
          fontSize: 9, borderRadius: 4, padding: '1px 5px', fontWeight: 600,
          transition: 'all 0.3s ease',
          color: serverHealthy ? '#16a34a' : '#dc2626',
          background: serverHealthy ? '#16a34a18' : '#dc262618',
        }}>
          PLAY
        </span>
        {projectRoot && (
          <span title="Project mode" style={{
            fontSize: 9, borderRadius: 4, padding: '1px 5px', fontWeight: 600,
            color: '#a855f7', background: '#a855f718',
            display: 'flex', alignItems: 'center', gap: 3,
          }}>
            <FolderGit2 size={10} /> .grpctestify
          </span>
        )}
      </div>

      <div style={{ flex: 1, minWidth: 8 }} />

      {}
      <div ref={dropdownRef} style={{ position: 'relative' }}>
        <button onClick={() => setShowDropdown(v => !v)} style={{
          display: 'flex', alignItems: 'center', gap: 4,
          padding: '5px 8px', fontSize: 11, borderRadius: 5,
          border: '1px solid var(--border)',
          background: 'var(--bg-primary)', color: 'var(--text-primary)',
          cursor: 'pointer', outline: 'none', maxWidth: 180,
          transition: 'border-color 0.15s ease',
        }}
          onMouseEnter={e => { e.currentTarget.style.borderColor = colors.accent; }}
          onMouseLeave={e => { e.currentTarget.style.borderColor = 'var(--border)'; }}
          title={activeEnv?.variables
            ? `${activeEnv.name}: ${Object.keys(activeEnv.variables).length} vars${(activeEnv.mutedVariables?.length || 0) > 0 ? `, ${activeEnv.mutedVariables?.filter(k => k in activeEnv.variables).length} muted` : ''}`
            : activeEnv ? activeEnv.name : 'No active environment'}
        >
          <span style={{
            overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
            color: activeEnvironment ? 'var(--text-primary)' : 'var(--text-muted)',
            flex: 1,
          }}>
            {activeEnvironment
              ? `${activeEnvironment}${Object.keys(activeEnv?.variables || {}).length > 0 ? ` (${Object.keys(activeEnv!.variables).length})` : ''}`
              : 'No environment'}
          </span>
          <ChevronDown size={12} style={{ flexShrink: 0 }} />
        </button>

        {showDropdown && (
          <div style={{
            position: 'absolute', top: '100%', left: 0, zIndex: 100,
            minWidth: 180, marginTop: 2,
            background: 'var(--bg-primary)', border: '1px solid var(--border)',
            borderRadius: 6, boxShadow: '0 4px 16px rgba(0,0,0,0.2)',
            overflow: 'hidden',
          }}>
            <div onClick={() => { setActiveEnvironment(null); setShowDropdown(false); }} style={{
              display: 'flex', alignItems: 'center', gap: 6, padding: '6px 10px', cursor: 'pointer', fontSize: 12,
              color: !activeEnvironment ? colors.accent : 'var(--text-muted)',
              fontWeight: !activeEnvironment ? 600 : 400,
              background: !activeEnvironment ? `${colors.accent}08` : 'transparent',
            }}
              onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-tertiary)'; }}
              onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; }}
            >
              <span style={{ width: 14 }} />
              <span>No environment</span>
            </div>
            {environments.map(env => {
              const isActive = activeEnvironment === env.name;
              const varCount = Object.keys(env.variables).length;
              const mutedCount = env.mutedVariables?.filter(k => k in env.variables).length || 0;
              const secCount = Object.entries(env.variables).filter(([, v]) => !v).length;
              return (
                <div key={env.name} onClick={() => { setActiveEnvironment(env.name); setShowDropdown(false); }}
                  title={`${varCount} var${varCount !== 1 ? 's' : ''}${secCount > 0 ? `, ${secCount} secret${secCount > 1 ? 's' : ''}` : ''}${mutedCount > 0 ? `, ${mutedCount} muted` : ''}`}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 6, padding: '6px 10px', cursor: 'pointer', fontSize: 12,
                    background: isActive ? `${colors.accent}10` : 'transparent',
                    color: isActive ? colors.accent : 'var(--text-primary)',
                    fontWeight: isActive ? 600 : 400,
                  }}
                  onMouseEnter={e => { e.currentTarget.style.background = isActive ? `${colors.accent}10` : 'var(--bg-tertiary)'; }}
                  onMouseLeave={e => { e.currentTarget.style.background = isActive ? `${colors.accent}10` : 'transparent'; }}
                >
                  <span style={{ width: 14, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
                    {isActive && <Check size={12} />}
                  </span>
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', fontSize: 12 }}>
                      {env.name}
                      {isActive && mutedCount > 0 && (
                        <span style={{ fontSize: 9, color: colors.warning, marginLeft: 4 }}>({mutedCount} muted)</span>
                      )}
                    </div>
                    <div style={{ fontSize: 9, color: 'var(--text-muted)', display: 'flex', gap: 4 }}>
                      <span>{varCount} var{varCount !== 1 ? 's' : ''}</span>
                      {secCount > 0 && <span style={{ color: `${colors.warning}99` }}>{secCount} secret{secCount !== 1 ? 's' : ''}</span>}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <button onClick={() => setShowEnvManager(true)} title="Manage environments"
        style={{ ...btn('ghost', 'sm'), color: activeEnvironment ? colors.accent : 'var(--text-muted)' }}>
        <Settings size={13} />
      </button>

      {}
      <div style={{ position: 'relative', display: 'flex', alignItems: 'center', gap: 4 }}>
        <div style={{ position: 'relative' }}>
          <input value={address} onChange={e => setAddress(e.target.value)}
            placeholder="host:port"
            title={
              hasVarPattern && activeEnv?.name
                ? `Resolves to: ${resolvedAddress}\n\nVariables:\n${
                    Object.entries(activeEnv.variables)
                      .filter(([k]) => address.includes(`{{${k}}}`))
                      .map(([k, v]) => `  ${k}=${v || '(secret)'}`)
                      .join('\n')
                  }`
                : activeEnv?.name
                  ? `Active env: "${activeEnv.name}" — {{KEY}} will be substituted`
                  : 'Use {{KEY}} patterns with an active environment'
            }
            style={{
              ...css.mono, padding: '5px 8px', fontSize: 12, borderRadius: 5,
              border: hasVarPattern && activeEnv
                ? `2px solid ${colors.accent}`
                : activeEnv
                  ? `1px solid ${colors.accent}60`
                  : '1px solid var(--border)',
              background: hasVarPattern && activeEnv ? `${colors.accent}08` : 'var(--bg-primary)',
              color: 'var(--text-primary)', outline: 'none', width: 170,
              transition: 'border-color 0.15s ease',
            }}
            onFocus={e => { e.currentTarget.style.borderColor = hasVarPattern ? colors.accent : (activeEnv ? `${colors.accent}80` : colors.accent); }}
            onBlur={e => { e.currentTarget.style.borderColor = hasVarPattern && activeEnv ? colors.accent : (activeEnv ? `${colors.accent}60` : 'var(--border)'); }}
          />
        </div>
        {hasVarPattern && activeEnv && resolvedAddress !== address && (
          <span style={{
            fontSize: 9, padding: '2px 6px', borderRadius: 4, whiteSpace: 'nowrap',
            background: `${colors.accent}18`, color: colors.accent,
            fontFamily: 'monospace', border: `1px solid ${colors.accent}30`,
          }} title={resolvedAddress}>
            {resolvedAddress}
          </span>
        )}
      </div>

      {}
      <select value={protocol} onChange={e => setProtocol(e.target.value as any)}
        style={{ padding: '5px 6px', fontSize: 12, borderRadius: 5, border: '1px solid var(--border)', background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none', cursor: 'pointer' }}
      >
        <option value="grpc">gRPC</option>
        <option value="grpc-web">gRPC-Web</option>
        <option value="connect">Connect</option>
      </select>

      {}
      <button onClick={() => setTls(!tls)} title={tls ? 'TLS on' : 'TLS off'}
        style={{ ...btn('ghost', 'sm'), color: tls ? colors.accent : 'var(--text-muted)' }}
      >
        {tls ? <Shield size={14} /> : <ShieldOff size={14} />}
      </button>

      {tls && (
        <label style={{ display: 'flex', alignItems: 'center', gap: 3, fontSize: 10, color: 'var(--text-muted)', cursor: 'pointer', userSelect: 'none' }}>
          <input type="checkbox" checked={tlsInsecure} onChange={e => setTlsInsecure(e.target.checked)} style={{ accentColor: colors.accent }} />
          insecure
        </label>
      )}

      {}
      <button onClick={reflect} disabled={reflectStatus === 'loading' || !address}
        title={reflectStatus === 'error' ? `Reflection failed: ${reflectError || 'unknown error'}` : "Discover services via reflection"}
        style={{ ...btn('ghost', 'sm'), opacity: reflectStatus === 'loading' || !address ? 0.5 : 1 }}
      >
        {reflectStatus === 'loading' ? <Loader2 size={14} className="animate-spin" />
          : reflectStatus === 'ok' ? <Check size={14} />
          : reflectStatus === 'error' ? <X size={14} />
          : <RefreshCw size={14} />}
        {reflectStatus === 'loading' ? '…' : 'Reflect'}
      </button>

      {}
      <button onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')}
        title={theme === 'dark' ? 'Light mode' : 'Dark mode'}
        style={btn('ghost', 'sm')}
        onMouseEnter={e => { e.currentTarget.style.color = 'var(--text-primary)'; }}
        onMouseLeave={e => { e.currentTarget.style.color = ''; }}
      >
        {theme === 'dark' ? <Sun size={14} /> : <Moon size={14} />}
      </button>

      {showEnvManager && <EnvironmentManager onClose={() => setShowEnvManager(false)} />}
    </header>
  );
}
