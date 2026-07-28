import { useState, useRef, useEffect, useMemo } from 'react';
import { useStore } from '../../lib/store';
import { useModal } from '../ui/ModalContext';
import { useToast } from '../ui/ToastContext';
import { BodyEditor } from './BodyEditor';
import { HeadersEditor } from './HeadersEditor';
import { EnvEditor } from './EnvEditor';
import { TabBar } from './TabBar';
import { btn, input, colors } from '../../lib/theme';
import { Play, Save, Square, ChevronDown, Loader2, ListChecks } from 'lucide-react';

function groupMethods(methods: { name: string; fullName: string; service: string }[]) {
  const map = new Map<string, { name: string; fullName: string }[]>();
  for (const m of methods) {
    const service = m.fullName.split('/')[0] || m.service;
    const group = map.get(service);
    if (group) group.push({ name: m.name, fullName: m.fullName });
    else map.set(service, [{ name: m.name, fullName: m.fullName }]);
  }
  return [...map.entries()];
}

export function matchesQuery(fullName: string, query: string) {
  const haystack = fullName.toLowerCase();
  return query.toLowerCase().split(/\s+/).filter(Boolean).every(token => haystack.includes(token));
}

export function RequestPanel() {
  const request = useStore(s => s.request);
  const setEndpoint = useStore(s => s.setEndpoint);
  const requestTab = useStore(s => s.requestTab);
  const setRequestTab = useStore(s => s.setRequestTab);
  const execute = useStore(s => s.execute);
  const cancel = useStore(s => s.cancel);
  const runTest = useStore(s => s.runTest);
  const runStatus = useStore(s => s.runStatus);
  const runMode = useStore(s => s.runMode);
  const setRunMode = useStore(s => s.setRunMode);
  const reflectionMethods = useStore(s => s.reflectionMethods);
  const address = useStore(s => s.address);

  const saveWorkspace = useStore(s => s.saveWorkspace);
  const saveWorkspaceAs = useStore(s => s.saveWorkspaceAs);
  const workspacePath = useStore(s => s.workspacePath);
  const modal = useModal();
  const toast = useToast();

  const reflectStatus = useStore(s => s.reflectStatus);
  const reflect = useStore(s => s.reflect);
  const [saving, setSaving] = useState(false);
  const [showDropdown, setShowDropdown] = useState(false);
  const [dropdownSearch, setDropdownSearch] = useState('');
  const [focusDropdownSearch, setFocusDropdownSearch] = useState(false);
  const [showModeMenu, setShowModeMenu] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const modeMenuRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const isExecuting = useStore(s => s.response?.status) === 'pending';
  const canExecute = !!request.endpoint && !isExecuting;

  
  const handleEndpointFocus = () => {
    if (reflectionMethods.length === 0 && reflectStatus === 'idle' && address) {
      reflect();
    }
  };

  
  const grouped = useMemo(() => groupMethods(reflectionMethods), [reflectionMethods]);

  
  const filteredDropdown = useMemo(() => {
    if (!dropdownSearch) return grouped;
    const q = dropdownSearch;
    return grouped
      .map(([svc, methods]) => [
        svc,
        methods.filter(m => matchesQuery(m.fullName, q)),
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

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (modeMenuRef.current && !modeMenuRef.current.contains(e.target as Node)) setShowModeMenu(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  const handleSelect = (fullName: string) => {
    setEndpoint(fullName);
    setShowDropdown(false);
    setDropdownSearch('');
  };

  const handleSave = async () => {
    if (!workspacePath) {
      const name = await modal.prompt('Save As', 'Save as:', 'untitled.gctf');
      if (!name) return;
      setSaving(true);
      try {
        await saveWorkspaceAs(name);
      } catch (err: any) {
        toast.error(err?.message || 'Save failed');
      } finally {
        setSaving(false);
      }
      return;
    }
    setSaving(true);
    try {
      await saveWorkspace();
    } catch (err: any) {
      toast.error(err?.message || 'Save failed');
    } finally {
      setSaving(false);
    }
  };

  return (
    <section>
      <TabBar />

      <div style={{ display: 'flex', gap: 6, marginBottom: 8 }}>
        <div style={{ flex: 1, position: 'relative' }}>
          <div style={{ display: 'flex', border: `1px solid var(--border)`, borderRadius: 6, overflow: 'hidden' }}>
            <input ref={inputRef} value={request.endpoint}
              onChange={e => { setEndpoint(e.target.value); setFocusDropdownSearch(false); setShowDropdown(true); setDropdownSearch(e.target.value); }}
              onFocus={() => { setFocusDropdownSearch(false); setShowDropdown(true); handleEndpointFocus(); }}
              placeholder="package.Service/Method"
              style={{ ...input, border: 'none', flex: 1, paddingRight: 8, fontFamily: 'monospace' }}
              onFocusCapture={e => { e.currentTarget.style.borderColor = colors.accent; }}
              onBlurCapture={e => { e.currentTarget.style.borderColor = 'var(--border)'; }}
            />
            <button onClick={() => { setFocusDropdownSearch(true); setShowDropdown(v => !v); }} style={{ ...btn('ghost', 'sm'), borderRadius: 0, borderLeft: '1px solid var(--border)' }} title="Select method">
              <ChevronDown size={14} />
            </button>
          </div>

          {showDropdown && reflectionMethods.length > 0 && (
            <div ref={dropdownRef} style={{
              position: 'absolute', top: '100%', left: 0, right: 0, zIndex: 100,
              background: 'var(--bg-secondary)', border: '1px solid var(--border)',
              borderRadius: 6, boxShadow: '0 4px 16px rgba(0,0,0,0.2)',
              maxHeight: 320, overflow: 'auto', marginTop: 2,
            }}>
              <div style={{ padding: '4px 6px', borderBottom: '1px solid var(--border)' }}>
                <input value={dropdownSearch} onChange={e => setDropdownSearch(e.target.value)} placeholder="Search package, service or method…" autoFocus={focusDropdownSearch}
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

        <div ref={modeMenuRef} style={{ position: 'relative', display: 'flex' }}>
          {isExecuting ? (
            <button onClick={cancel} style={{ ...btn('danger'), borderTopRightRadius: 0, borderBottomRightRadius: 0 }}>
              <Square size={14} /> Cancel
            </button>
          ) : runMode === 'run' ? (
            <button onClick={runTest} disabled={!workspacePath || runStatus === 'running'}
              title={workspacePath ? 'Run the saved .gctf file — ASSERTS/EXTRACT included, same engine `grpctestify run` uses' : 'Save this as a collection file first'}
              style={{
                ...btn('primary'),
                background: colors.accent,
                borderTopRightRadius: 0, borderBottomRightRadius: 0,
                opacity: workspacePath && runStatus !== 'running' ? 1 : 0.45,
                cursor: workspacePath && runStatus !== 'running' ? 'pointer' : 'not-allowed',
              }}>
              {runStatus === 'running' ? <Loader2 size={14} className="animate-spin" /> : <ListChecks size={14} />}
              Run
            </button>
          ) : (
            <button onClick={execute}
              aria-disabled={!canExecute}
              style={{
                ...btn('primary'),
                background: colors.accent,
                borderTopRightRadius: 0, borderBottomRightRadius: 0,
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
              <Play size={14} fill="#fff" /> Execute
            </button>
          )}

          <button onClick={() => setShowModeMenu(v => !v)} disabled={isExecuting} title="Choose mode" style={{
            ...btn('primary'),
            background: colors.accent,
            borderLeft: '1px solid rgba(255,255,255,0.35)',
            borderTopLeftRadius: 0, borderBottomLeftRadius: 0,
            padding: '0 6px',
            opacity: isExecuting ? 0.45 : 1,
            cursor: isExecuting ? 'not-allowed' : 'pointer',
          }}>
            <ChevronDown size={14} />
          </button>

          {showModeMenu && (
            <div style={{
              position: 'absolute', top: '100%', left: 0, zIndex: 100, marginTop: 2, minWidth: 230,
              background: 'var(--bg-secondary)', border: '1px solid var(--border)', borderRadius: 6,
              boxShadow: '0 4px 16px rgba(0,0,0,0.2)', overflow: 'hidden',
            }}>
              {([
                { mode: 'execute' as const, icon: <Play size={13} />, label: 'Execute', desc: 'Send using the live editor state' },
                { mode: 'run' as const, icon: <ListChecks size={13} />, label: 'Run', desc: 'Run the saved .gctf file — ASSERTS/EXTRACT included' },
              ]).map(opt => (
                <div key={opt.mode} onClick={() => { setRunMode(opt.mode); setShowModeMenu(false); }} style={{
                  display: 'flex', alignItems: 'flex-start', gap: 8, padding: '8px 10px', cursor: 'pointer',
                  background: runMode === opt.mode ? 'var(--bg-tertiary)' : 'transparent',
                }}
                  onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-tertiary)'; }}
                  onMouseLeave={e => { e.currentTarget.style.background = runMode === opt.mode ? 'var(--bg-tertiary)' : 'transparent'; }}>
                  <span style={{ marginTop: 1 }}>{opt.icon}</span>
                  <div>
                    <div style={{ fontSize: 12, fontWeight: 600 }}>{opt.label}</div>
                    <div style={{ fontSize: 10, color: 'var(--text-muted)' }}>{opt.desc}</div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        <button onClick={handleSave} disabled={saving} style={btn()}
          onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-secondary)'; }}
          onMouseLeave={e => { e.currentTarget.style.background = ''; }}>
          <Save size={14} /> {saving ? 'Saving…' : (workspacePath ? 'Save' : 'Save As…')}
        </button>
      </div>

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
