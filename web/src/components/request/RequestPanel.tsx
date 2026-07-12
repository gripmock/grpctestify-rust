import { useState, useCallback, useRef, useEffect, useMemo } from 'react';
import { useStore } from '../../lib/store';
import { BodyEditor } from './BodyEditor';
import { HeadersEditor } from './HeadersEditor';
import { EnvEditor } from './EnvEditor';
import { TabBar } from './TabBar';
import { btn, input, colors } from '../../lib/theme';
import { Play, Save, Copy, Square, Sparkles, ChevronDown, Loader2 } from 'lucide-react';

function groupMethods(methods: { name: string; fullName: string; service: string }[]) {
  const map = new Map<string, { name: string; fullName: string }[]>();
  for (const m of methods) {
    if (!map.has(m.service)) map.set(m.service, []);
    map.get(m.service)!.push({ name: m.name, fullName: m.fullName });
  }
  return [...map.entries()];
}

export function RequestPanel() {
  const request = useStore(s => s.request);
  const setEndpoint = useStore(s => s.setEndpoint);
  const setRequestBody = useStore(s => s.setRequestBody);
  const requestTab = useStore(s => s.requestTab);
  const setRequestTab = useStore(s => s.setRequestTab);
  const execute = useStore(s => s.execute);
  const cancel = useStore(s => s.cancel);
  const getGrpcurlCommand = useStore(s => s.getGrpcurlCommand);
  const reflectionMethods = useStore(s => s.reflectionMethods);
  const address = useStore(s => s.address);
  const protocol = useStore(s => s.protocol);
  const tls = useStore(s => s.tls);
  const tlsInsecure = useStore(s => s.tlsInsecure);
  const selectedCollection = useStore(s => s.selectedCollection);

  const saveWorkspace = useStore(s => s.saveWorkspace);
  const workspacePath = useStore(s => s.workspacePath);

  const reflectStatus = useStore(s => s.reflectStatus);
  const reflect = useStore(s => s.reflect);
  const [saving, setSaving] = useState(false);
  const [grpurlError, setGrpurlError] = useState<string | null>(null);
  const [grpcurlCopied, setGrpcurlCopied] = useState(false);
  const [showDropdown, setShowDropdown] = useState(false);
  const [dropdownSearch, setDropdownSearch] = useState('');
  const [filling, setFilling] = useState(false);
  const [fillError, setFillError] = useState<string | null>(null);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const isExecuting = useStore(s => s.response?.status) === 'pending';
  const canExecute = !!request.endpoint && !isExecuting;

  
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
        e.preventDefault();
        if (canExecute) execute();
      }
    };
    document.addEventListener('keydown', handler);
    return () => document.removeEventListener('keydown', handler);
  }, [canExecute]);

  
  const handleEndpointFocus = () => {
    if (reflectionMethods.length === 0 && reflectStatus === 'idle' && address) {
      reflect();
    }
  };

  
  const grouped = useMemo(() => groupMethods(reflectionMethods), [reflectionMethods]);

  
  const filteredDropdown = useMemo(() => {
    if (!dropdownSearch) return grouped;
    const q = dropdownSearch.toLowerCase();
    return grouped
      .map(([svc, methods]) => [
        svc,
        methods.filter(m => m.name.toLowerCase().includes(q) || m.fullName.toLowerCase().includes(q)),
      ] as const)
      .filter(([_, methods]) => methods.length > 0);
  }, [grouped, dropdownSearch]);

  
  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node) && inputRef.current && !inputRef.current.contains(e.target as Node)) setShowDropdown(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  const handleSelect = (fullName: string) => {
    setEndpoint(fullName);
    setShowDropdown(false);
    setDropdownSearch('');
  };

  const handleAutoFill = async () => {
    if (!request.endpoint) return;
    setFilling(true);
    setFillError(null);
    try {
      const res = await fetch('/api/schema-fill', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          address,
          endpoint: request.endpoint,
          tls: tls || undefined,
          tls_insecure: tls ? tlsInsecure : undefined,
          collection_path: selectedCollection || undefined,
          protocol: protocol || undefined,
        }),
      });
      const data = await res.json();
      if (data.error) {
        setFillError(data.error);
        return;
      }
      setRequestBody(0, JSON.stringify(data.schema, null, 2));
    } catch (err: any) {
      setFillError(err?.message || String(err));
    } finally {
      setFilling(false);
    }
  };

  const handleSave = async () => {
    setSaving(true);
    setGrpurlError(null);
    try {
      await saveWorkspace();
    } catch (err: any) {
      setGrpurlError(err?.message || 'Save failed');
    } finally {
      setSaving(false);
    }
  };

  const handleGetGrpcurl = useCallback(async () => {
    setGrpurlError(null);
    try {
      const cmd = await getGrpcurlCommand();
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(cmd);
      } else {
        const ta = document.createElement('textarea');
        ta.value = cmd;
        ta.style.position = 'fixed';
        ta.style.opacity = '0';
        document.body.appendChild(ta);
        ta.select();
        document.execCommand('copy');
        document.body.removeChild(ta);
      }
      setGrpcurlCopied(true);
      setTimeout(() => setGrpcurlCopied(false), 2000);
    } catch (err: any) {
      setGrpurlError(err?.message || String(err));
      setTimeout(() => setGrpurlError(null), 4000);
    }
  }, [getGrpcurlCommand]);

  return (
    <section>
      {}
      <TabBar />

      {}
      <div style={{ display: 'flex', gap: 6, marginBottom: 8 }}>
        {}
        <div style={{ flex: 1, position: 'relative' }}>
          {}
          <div style={{ display: 'flex', border: `1px solid var(--border)`, borderRadius: 6, overflow: 'hidden' }}>
            <input ref={inputRef} value={request.endpoint}
              onChange={e => { setEndpoint(e.target.value); setShowDropdown(true); setDropdownSearch(e.target.value); }}
              onFocus={() => { setShowDropdown(true); handleEndpointFocus(); }}
              placeholder="package.Service/Method"
              style={{ ...input, border: 'none', flex: 1, paddingRight: 8, fontFamily: 'monospace' }}
              onFocusCapture={e => { e.currentTarget.style.borderColor = colors.accent; }}
              onBlurCapture={e => { e.currentTarget.style.borderColor = 'var(--border)'; }}
            />
            <button onClick={() => setShowDropdown(v => !v)} style={{ ...btn('ghost', 'sm'), borderRadius: 0, borderLeft: '1px solid var(--border)' }} title="Select method">
              <ChevronDown size={14} />
            </button>
          </div>

          {}
          {showDropdown && reflectionMethods.length > 0 && (
            <div ref={dropdownRef} style={{
              position: 'absolute', top: '100%', left: 0, right: 0, zIndex: 100,
              background: 'var(--bg-secondary)', border: '1px solid var(--border)',
              borderRadius: 6, boxShadow: '0 4px 16px rgba(0,0,0,0.2)',
              maxHeight: 320, overflow: 'auto', marginTop: 2,
            }}>
              {}
              <div style={{ padding: '4px 6px', borderBottom: '1px solid var(--border)' }}>
                <input value={dropdownSearch} onChange={e => setDropdownSearch(e.target.value)} placeholder="Search…" autoFocus
                  style={{ width: '100%', border: 'none', background: 'transparent', fontSize: 12, color: 'var(--text-primary)', outline: 'none', padding: '4px' }} />
              </div>

              {filteredDropdown.length === 0 && (
                <div style={{ padding: 8, fontSize: 12, color: 'var(--text-muted)', textAlign: 'center' }}>No methods found</div>
              )}

              {filteredDropdown.map(([svc, methods]) => (
                <div key={svc}>
                  <div style={{ padding: '4px 8px', fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.5px', background: 'var(--bg-tertiary)' }}>
                    {svc}
                  </div>
                  {methods.map(m => (
                    <div key={m.fullName} onClick={() => handleSelect(m.fullName)} style={{
                      padding: '5px 8px 5px 12px', cursor: 'pointer', fontSize: 12, fontFamily: 'monospace',
                      transition: 'background 0.1s',
                    }}
                      onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-tertiary)'; }}
                      onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; }}
                    >
                      {m.name}
                    </div>
                  ))}
                </div>
              ))}
            </div>
          )}
        </div>

        {}
        <button onClick={handleAutoFill} disabled={!request.endpoint || filling} style={{
          ...btn(), opacity: request.endpoint && !filling ? 1 : 0.4,
          cursor: request.endpoint && !filling ? 'pointer' : 'not-allowed',
        }} title="Auto-fill body from proto">
          {filling ? <Loader2 size={14} className="animate-spin" /> : <Sparkles size={14} />}
          Auto Fill
        </button>
        {fillError && <span style={{ fontSize: 10, color: 'var(--error)', maxWidth: 120, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{fillError}</span>}

        {isExecuting ? (
          <button onClick={cancel} style={btn('danger')}><Square size={16} /> Cancel</button>
        ) : (
          <button onClick={execute}
            aria-disabled={!canExecute}
            style={{
              ...btn('primary'),
              background: colors.accent,
              cursor: canExecute ? 'pointer' : 'not-allowed',
              opacity: canExecute ? 1 : 0.45,
            }}
            onMouseEnter={e => {
              if (canExecute) {
                e.currentTarget.style.background = colors.accentHover;
                e.currentTarget.style.transform = 'translateY(-1px)';
              }
            }}
            onMouseLeave={e => {
              if (canExecute) {
                e.currentTarget.style.background = colors.accent;
                e.currentTarget.style.transform = '';
              }
            }}>
            <Play size={16} fill="#fff" /> Execute
          </button>
        )}

        <button onClick={handleGetGrpcurl} disabled={!request.endpoint || isExecuting} style={{
          ...btn(), opacity: request.endpoint && !isExecuting ? 1 : 0.4, cursor: request.endpoint && !isExecuting ? 'pointer' : 'not-allowed',
        }}
          onMouseEnter={e => { if (request.endpoint) e.currentTarget.style.background = 'var(--bg-secondary)'; }}
          onMouseLeave={e => { e.currentTarget.style.background = ''; }}>
          <Copy size={14} /> {grpcurlCopied ? 'Copied!' : 'grpcurl'}
        </button>

        <button onClick={handleSave} disabled={saving} style={btn()}
          onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-secondary)'; }}
          onMouseLeave={e => { e.currentTarget.style.background = ''; }}>
          <Save size={14} /> {saving ? 'Saving…' : (workspacePath ? 'Save' : 'Save As…')}
        </button>
      </div>

      {grpurlError && (
        <div style={{
          fontSize: 11, color: 'var(--error)', marginBottom: 6, padding: '4px 8px',
          background: 'var(--error-bg, rgba(239,68,68,0.08))', borderRadius: 4,
        }}>
          {grpurlError}
        </div>
      )}

      {}
      <div style={{ display: 'flex', borderBottom: '1px solid var(--border)', marginBottom: 8 }}>
        {(['body', 'headers', 'env'] as const).map(tab => (
          <button key={tab} onClick={() => setRequestTab(tab)} style={{
            padding: '5px 14px', fontSize: 12, cursor: 'pointer', border: 'none', background: 'none',
            transition: 'color 0.15s', color: requestTab === tab ? 'var(--accent)' : 'var(--text-secondary)',
            fontWeight: requestTab === tab ? 600 : 400,
            borderBottom: requestTab === tab ? '2px solid var(--accent)' : '2px solid transparent',
          }}>
            {tab === 'body' ? 'Request Body' : tab === 'headers' ? 'Headers' : 'Environment'}
          </button>
        ))}
      </div>

      {requestTab === 'body' && <BodyEditor />}
      {requestTab === 'headers' && <HeadersEditor />}
      {requestTab === 'env' && <EnvEditor />}
    </section>
  );
}
