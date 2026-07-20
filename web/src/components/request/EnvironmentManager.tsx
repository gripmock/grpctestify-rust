import { useState, useEffect, useCallback, useMemo } from 'react';
import { useStore } from '../../lib/store';
import { useModal } from '../ui/ModalContext';
import { useToast } from '../ui/ToastContext';
import type { Environment, EnvLocalStatus } from '../../lib/types';
import { btn, colors, css } from '../../lib/theme';
import { Plus, Pencil, Trash2, Copy, Check, X, Settings, FolderGit2, Globe, FileText, Lock, FilePlus2, Eye, EyeOff, Search, ArrowLeft, Shield, ShieldOff, Info } from 'lucide-react';

interface Props { onClose: () => void }



function DotEnvEditor({ value, onChange, onSave, canSave, saveLabel, hint, placeholder, onDelete, deleteLabel }: {
  value: string; onChange: (v: string) => void;
  onSave: () => void; canSave: boolean;
  saveLabel: string; hint: string; placeholder?: string;
  onDelete?: () => void; deleteLabel?: string;
}) {
  return (
    <div>
      <textarea value={value} onChange={e => onChange(e.target.value)} placeholder={placeholder}
        style={{
          width: '100%', height: 220, fontFamily: 'monospace', fontSize: 12, padding: 8,
          border: '1px solid var(--border)', borderRadius: 6, resize: 'vertical',
          background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none',
          lineHeight: 1.5, tabSize: 2, boxSizing: 'border-box',
        }} spellCheck={false}
      />
      <div style={{ display: 'flex', gap: 6, marginTop: 6, flexWrap: 'wrap', alignItems: 'center' }}>
        <button onClick={onSave} disabled={!canSave} style={{ ...btn('primary'), fontSize: 12, opacity: canSave ? 1 : 0.5 }}>
          <Check size={12} /> {saveLabel}
        </button>
        {onDelete && <button onClick={onDelete} style={{ ...btn('danger', 'sm'), fontSize: 12 }}><Trash2 size={12} /> {deleteLabel}</button>}
        <span style={{ fontSize: 10, color: 'var(--text-muted)' }}>{hint}</span>
      </div>
      <div style={{ fontSize: 10, color: 'var(--text-muted)', marginTop: 3 }}>Format: KEY=VALUE, one per line. # for comments.</div>
    </div>
  );
}



function VarRow({ k, v, i, editVars, setEditVars, editMuted, setEditMuted, showRemove, children }: {
  k: string; v: string; i: number;
  editVars: [string, string][]; setEditVars: (v: [string, string][]) => void;
  editMuted: Set<string>; setEditMuted: (v: Set<string>) => void;
  showRemove: boolean;
  children?: React.ReactNode;
}) {
  const isMuted = k ? editMuted.has(k) : false;
  const set = (nk: string, nv: string) => {
    const next = [...editVars];
    next[i] = [nk, nv];
    setEditVars(next);
  };
  return (
    <div style={{ display: 'flex', gap: 4, marginBottom: 3, opacity: isMuted ? 0.45 : 1, alignItems: 'center' }}>
      {k ? (
        <button onClick={() => { const n = new Set(editMuted); if (isMuted) n.delete(k); else n.add(k); setEditMuted(n); }}
          style={{ ...btn('ghost', 'sm'), padding: 3, color: isMuted ? colors.warning : 'var(--text-muted)', flexShrink: 0 }}
          title={isMuted ? `"${k}" muted — excluded from {{KEY}} substitution` : `Mute "${k}" — exclude from substitution`}>
          {isMuted ? <EyeOff size={12} /> : <Eye size={12} />}
        </button>
      ) : <span style={{ width: 24 }} />}
      {children}
      <input value={k} onChange={e => set(e.target.value, v)} placeholder="KEY"
        style={{ flex: 1, ...css.mono, padding: '4px 6px', fontSize: 11, borderRadius: 4, border: '1px solid var(--border)', background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none' }}
      />
      <input value={v} onChange={e => set(k, e.target.value)} placeholder="value"
        style={{ flex: 2, ...css.mono, padding: '4px 6px', fontSize: 11, borderRadius: 4, border: '1px solid var(--border)', background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none' }}
      />
      {showRemove && (
        <button onClick={() => setEditVars(editVars.filter((_, j) => j !== i))} style={{ ...btn('ghost', 'sm'), padding: 3, flexShrink: 0 }} title="Remove">
          <X size={11} />
        </button>
      )}
    </div>
  );
}



type View = { kind: 'list' } | { kind: 'edit'; name: string } | { kind: 'new' };



export function EnvironmentManager({ onClose }: Props) {
  const environments = useStore(s => s.environments);
  const addEnvironment = useStore(s => s.addEnvironment);
  const updateEnvironment = useStore(s => s.updateEnvironment);
  const deleteEnvironment = useStore(s => s.deleteEnvironment);
  const activeEnvironment = useStore(s => s.activeEnvironment);
  const setActiveEnvironment = useStore(s => s.setActiveEnvironment);
  const projectRoot = useStore(s => s.projectRoot);
  const projectEnvNames = useStore(s => s.projectEnvNames);
  const fetchProjectEnv = useStore(s => s.fetchProjectEnv);
  const saveProjectEnv = useStore(s => s.saveProjectEnv);
  const fetchProjectEnvLocal = useStore(s => s.fetchProjectEnvLocal);
  const saveProjectEnvLocal = useStore(s => s.saveProjectEnvLocal);
  const deleteProjectEnvLocal = useStore(s => s.deleteProjectEnvLocal);

  const modal = useModal();
  const toast = useToast();

  const [tab, setTab] = useState<'project' | 'browser'>(projectRoot ? 'project' : 'browser');
  const [view, setView] = useState<View>({ kind: 'list' });
  const [envSearch, setEnvSearch] = useState('');

  
  const [editName, setEditName] = useState('');
  const [editVars, setEditVars] = useState<[string, string][]>([['', '']]);
  const [editMuted, setEditMuted] = useState<Set<string>>(new Set());
  const [editSecretKeys, setEditSecretKeys] = useState<Set<string>>(new Set());
  const [showSecretValues, setShowSecretValues] = useState(false);

  
  const [selectedEnv, setSelectedEnv] = useState<string | null>(null);
  const [sharedContent, setSharedContent] = useState('');
  const [localContent, setLocalContent] = useState('');
  const [localStatus, setLocalStatus] = useState<EnvLocalStatus | null>(null);
  const [editMode, setEditMode] = useState<'shared' | 'local'>('shared');
  const [loadingEnv, setLoadingEnv] = useState(false);
  const [dirtyShared, setDirtyShared] = useState(false);
  const [dirtyLocal, setDirtyLocal] = useState(false);
  const [newEnvName, setNewEnvName] = useState('');

  
  const filteredEnvs = useMemo(() => {
    if (!envSearch) return environments;
    const q = envSearch.toLowerCase();
    return environments.filter(e => e.name.toLowerCase().includes(q));
  }, [environments, envSearch]);

  
  const openEdit = (env: Environment) => {
    setView({ kind: 'edit', name: env.name });
    setEditName(env.name);
    const vars = Object.entries(env.variables);
    setEditVars(vars.concat([['', '']]));
    setEditMuted(new Set(env.mutedVariables || []));
    setEditSecretKeys(new Set(vars.filter(([, v]) => !v).map(([k]) => k)));
    setShowSecretValues(false);
  };

  const openNew = () => {
    setView({ kind: 'new' });
    setEditName(''); setEditVars([['', '']]);
    setEditMuted(new Set()); setEditSecretKeys(new Set()); setShowSecretValues(false);
  };

  
  const saveEdit = () => {
    if (!editName.trim()) return;
    const vars = Object.fromEntries(editVars.filter(([k]) => k.trim()));
    const muted = [...editMuted].filter(k => k in vars);
    const env: Environment = { name: editName.trim(), variables: vars };
    if (muted.length > 0) env.mutedVariables = muted;
    if (view.kind === 'new') addEnvironment(env);
    else if (view.kind === 'edit') updateEnvironment(view.name, env);
    setView({ kind: 'list' });
  };

  
  const loadEnv = useCallback(async (name: string) => {
    setLoadingEnv(true); setSelectedEnv(name);
    setDirtyShared(false); setDirtyLocal(false); setNewEnvName('');
    try {
      const [shared, local] = await Promise.all([
        fetchProjectEnv(name).catch(() => ''),
        fetchProjectEnvLocal(name),
      ]);
      setSharedContent(shared || `# .env.${name}\nGRPC_ADDRESS=\n`);
      setLocalContent(local.content || '');
      setLocalStatus(local); setEditMode('shared');
    } catch {  }
    setLoadingEnv(false);
  }, [fetchProjectEnv, fetchProjectEnvLocal]);

  useEffect(() => {
    if (tab === 'project' && projectEnvNames.length > 0 && !selectedEnv) loadEnv(projectEnvNames[0]);
  }, [tab, projectEnvNames, selectedEnv, loadEnv]);

  const handleSaveShared = async () => {
    if (!selectedEnv) return;
    try { await saveProjectEnv(selectedEnv, sharedContent); setDirtyShared(false); } catch (err: any) { toast.error(err?.message); }
  };
  const handleSaveLocal = async () => {
    if (!selectedEnv) return;
    try { await saveProjectEnvLocal(selectedEnv, localContent); setLocalStatus({ exists: true, content: localContent }); setDirtyLocal(false); } catch (err: any) { toast.error(err?.message); }
  };
  const handleDeleteLocal = async () => {
    if (!selectedEnv || !await modal.confirm('Delete', 'Delete local overrides for this environment?')) return;
    try { await deleteProjectEnvLocal(selectedEnv); setLocalContent(''); setLocalStatus({ exists: false, content: null }); setDirtyLocal(false); } catch {  }
  };
  const handleCreateEnv = async () => {
    const name = newEnvName.trim(); if (!name) return;
    try {
      await saveProjectEnv(name, `# .env.${name}\nGRPC_ADDRESS=\n`);
      const st = useStore.getState();
      useStore.setState({ projectEnvNames: [...st.projectEnvNames, name].sort() });
      setNewEnvName(''); loadEnv(name);
    } catch (err: any) { toast.error(err?.message); }
  };
  const hasLocalOverrides = localStatus?.exists || dirtyLocal;

  return (
    <div style={{
      position: 'fixed', inset: 0, zIndex: 1000,
      display: 'flex', alignItems: 'center', justifyContent: 'center',
      background: 'rgba(0,0,0,0.4)',
    }} onClick={onClose}>
      <div style={{
        background: 'var(--bg-primary)', borderRadius: 10,
        border: '1px solid var(--border)', boxShadow: '0 8px 32px rgba(0,0,0,0.2)',
        width: projectRoot ? 720 : 540, maxHeight: '85vh', display: 'flex', flexDirection: 'column',
      }} onClick={e => e.stopPropagation()}>

        <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '12px 16px', borderBottom: '1px solid var(--border)', flexShrink: 0 }}>
          {view.kind !== 'list' && <button onClick={() => setView({ kind: 'list' })} style={btn('ghost', 'sm')}><ArrowLeft size={14} /></button>}
          <Settings size={16} />
          <span style={{ fontSize: 14, fontWeight: 600 }}>
            {view.kind === 'edit' ? `Edit: ${view.name}` : view.kind === 'new' ? 'New Environment' : 'Environments'}
          </span>
          {view.kind === 'list' && <span style={{ fontSize: 11, color: 'var(--text-muted)' }}>{environments.length}</span>}
          <span style={{ flex: 1 }} />
          <button onClick={onClose} style={btn('ghost', 'sm')}><X size={14} /></button>
        </div>

        {view.kind === 'list' && projectRoot && (
          <nav style={{ display: 'flex', borderBottom: '1px solid var(--border)', flexShrink: 0 }}>
            {(['project', 'browser'] as const).map(t => (
              <button key={t} onClick={() => setTab(t)} style={{
                flex: 1, display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 4,
                padding: '7px 4px', fontSize: 11, cursor: 'pointer', border: 'none', background: 'none',
                fontWeight: tab === t ? 600 : 400,
                color: tab === t ? 'var(--accent)' : 'var(--text-secondary)',
                borderBottom: tab === t ? '2px solid var(--accent)' : '2px solid transparent',
              }}>
                {t === 'project' ? <FolderGit2 size={13} /> : <Globe size={13} />}
                {t === 'project' ? 'Project (.env files)' : 'Browser local'}
              </button>
            ))}
          </nav>
        )}

        <div style={{ flex: 1, overflow: 'auto', padding: 12 }}>

          {(view.kind === 'edit' || view.kind === 'new') && (
            <div>
              <div style={{ marginBottom: 8 }}>
                <div style={{ fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', marginBottom: 3 }}>Name</div>
                <input value={editName} onChange={e => setEditName(e.target.value)} placeholder="my-environment"
                  style={{ width: '100%', padding: '6px 8px', fontSize: 13, borderRadius: 5, border: '1px solid var(--border)', background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none', boxSizing: 'border-box', fontFamily: 'monospace' }} />
              </div>

              <div style={{
                padding: 8, borderRadius: 6, border: '1px solid var(--border)', marginBottom: 10,
                background: 'var(--bg-primary)',
              }}>
                <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-muted)', marginBottom: 3, display: 'flex', alignItems: 'center', gap: 4 }}>
                  <Eye size={12} /> Variables <span style={{ fontSize: 9, color: 'var(--text-muted)', fontWeight: 400 }}>— shared, visible in git</span>
                </div>
                {editVars.map(([k, v], i) => {
                  if (editSecretKeys.has(k)) return null;
                  return <VarRow key={i} k={k} v={v} i={i} editVars={editVars} setEditVars={setEditVars} editMuted={editMuted} setEditMuted={setEditMuted} showRemove={true}>
                    <button onClick={() => { const n = new Set(editSecretKeys); n.add(k); setEditSecretKeys(n); }}
                      style={{ ...btn('ghost', 'sm'), padding: 3, color: 'var(--text-muted)', flexShrink: 0 }} title={`Move "${k}" to secrets`}>
                      <Shield size={11} />
                    </button>
                  </VarRow>;
                })}
                <button onClick={() => setEditVars([...editVars, ['', '']])} style={{ ...btn('ghost', 'sm'), fontSize: 11, marginTop: 3, color: colors.accent }}>
                  <Plus size={11} /> Add variable
                </button>
              </div>

              <div style={{
                padding: 8, borderRadius: 6, border: `1px solid ${colors.warning}30`, marginBottom: 10,
                background: `${colors.warning}04`,
              }}>
                <div style={{ fontSize: 11, fontWeight: 600, color: colors.warning, marginBottom: 3, display: 'flex', alignItems: 'center', gap: 4 }}>
                  <Lock size={12} /> Secrets <span style={{ fontSize: 9, fontWeight: 400 }}>— your private values (gitignored)</span>
                  {editSecretKeys.size > 0 && (
                    <button onClick={() => setShowSecretValues(v => !v)} style={{ ...btn('ghost', 'sm'), fontSize: 9, padding: '1px 5px', marginLeft: 'auto' }}>
                      {showSecretValues ? <EyeOff size={11} /> : <Eye size={11} />} {showSecretValues ? 'Hide' : 'Show values'}
                    </button>
                  )}
                </div>

                {editSecretKeys.size === 0 && (
                  <div style={{ fontSize: 11, color: 'var(--text-muted)', padding: '4px 0' }}>
                    Click <Shield size={10} style={{ display: 'inline' }} /> on any variable above to mark it as a secret.
                  </div>
                )}

                {editVars.map(([k, v], i) => {
                  if (!k || !editSecretKeys.has(k)) return null;
                  return <VarRow key={i} k={k} v={showSecretValues ? v : (v ? '••••••' : '')} i={i} editVars={editVars} setEditVars={setEditVars} editMuted={editMuted} setEditMuted={setEditMuted} showRemove={true}>
                    <button onClick={() => { const n = new Set(editSecretKeys); n.delete(k); setEditSecretKeys(n); }}
                      style={{ ...btn('ghost', 'sm'), padding: 3, color: colors.warning, flexShrink: 0 }} title={`Move "${k}" back to variables`}>
                      <ShieldOff size={11} />
                    </button>
                  </VarRow>;
                })}
              </div>

              {editMuted.size > 0 && (
                <div style={{ fontSize: 10, color: colors.warning, marginBottom: 8, display: 'flex', alignItems: 'center', gap: 4 }}>
                  <Info size={10} /> {editMuted.size} variable{editMuted.size > 1 ? 's' : ''} muted — excluded from <code style={{ background: `${colors.warning}12`, padding: '0 3px', borderRadius: 2, fontSize: 10 }}>{'{{KEY}}'}</code> substitution
                </div>
              )}

              <div style={{ display: 'flex', gap: 6, marginTop: 10 }}>
                <button onClick={saveEdit} disabled={!editName.trim()} style={{ ...btn('primary'), fontSize: 12, opacity: editName.trim() ? 1 : 0.5 }}>
                  <Check size={12} /> {view.kind === 'new' ? 'Create Environment' : 'Save Changes'}
                </button>
                <button onClick={() => setView({ kind: 'list' })} style={{ ...btn(), fontSize: 12 }}>Cancel</button>
              </div>
            </div>
          )}

          {view.kind === 'list' && tab === 'project' && projectRoot && (
            <div style={{ display: 'flex', gap: 12, minHeight: 320 }}>
              <div style={{ width: 180, flexShrink: 0, display: 'flex', flexDirection: 'column', gap: 4 }}>
                <div style={{ fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.5px' }}>.env files</div>
                {projectEnvNames.length === 0 && <div style={{ fontSize: 11, color: 'var(--text-muted)', padding: '8px 0' }}>No .env files yet</div>}
                <div style={{ flex: 1, overflow: 'auto' }}>
                  {projectEnvNames.map(name => (
                    <div key={name} onClick={async () => { if (!dirtyShared || await modal.confirm('Discard', 'Discard changes?')) loadEnv(name); }} style={{
                      display: 'flex', alignItems: 'center', gap: 4, padding: '5px 8px', borderRadius: 5,
                      cursor: 'pointer', fontSize: 12, marginBottom: 2,
                      background: selectedEnv === name ? colors.accent + '18' : 'transparent',
                      color: selectedEnv === name ? 'var(--accent)' : 'var(--text-primary)',
                      fontWeight: selectedEnv === name ? 600 : 400,
                    }}>
                      <FileText size={12} />
                      <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{name}</span>
                      {activeEnvironment === name && <Check size={10} style={{ color: colors.accent }} />}
                    </div>
                  ))}
                </div>
                <div style={{ display: 'flex', gap: 4 }}>
                  <input value={newEnvName} onChange={e => setNewEnvName(e.target.value)} placeholder="new env name"
                    style={{ flex: 1, padding: '3px 6px', fontSize: 11, fontFamily: 'monospace', border: '1px solid var(--border)', borderRadius: 4, background: 'var(--bg-primary)', color: 'var(--text-primary)', outline: 'none' }}
                    onKeyDown={e => { if (e.key === 'Enter') handleCreateEnv(); }} />
                  <button onClick={handleCreateEnv} disabled={!newEnvName.trim()} style={{ ...btn('primary', 'sm'), fontSize: 11, opacity: newEnvName.trim() ? 1 : 0.4 }}>
                    <FilePlus2 size={12} />
                  </button>
                </div>
              </div>

              <div style={{ flex: 1, minWidth: 0 }}>
                {loadingEnv && <div style={{ fontSize: 12, color: 'var(--text-muted)', padding: 20 }}>Loading…</div>}
                {!loadingEnv && selectedEnv && (
                  <>
                    <div style={{ display: 'flex', gap: 0, marginBottom: 8, borderBottom: '1px solid var(--border)' }}>
                      <button onClick={() => setEditMode('shared')} style={{
                        padding: '5px 12px', fontSize: 11, cursor: 'pointer', border: 'none', background: 'none',
                        color: editMode === 'shared' ? 'var(--accent)' : 'var(--text-secondary)',
                        fontWeight: editMode === 'shared' ? 600 : 400,
                        borderBottom: editMode === 'shared' ? '2px solid var(--accent)' : '2px solid transparent',
                      }}>
                        <FileText size={12} style={{ marginRight: 4 }} />
                        Shared <span style={{ fontSize: 9, opacity: 0.6 }}>git</span>
                      </button>
                      <button onClick={() => setEditMode('local')} style={{
                        padding: '5px 12px', fontSize: 11, cursor: 'pointer', border: 'none', background: 'none',
                        color: editMode === 'local' ? 'var(--accent)' : 'var(--text-secondary)',
                        fontWeight: editMode === 'local' ? 600 : 400,
                        borderBottom: editMode === 'local' ? '2px solid var(--accent)' : '2px solid transparent',
                      }}>
                        <Lock size={12} style={{ marginRight: 4 }} />
                        Local overrides{hasLocalOverrides ? ' 🟡' : ''} <span style={{ fontSize: 9, opacity: 0.6 }}>local</span>
                      </button>
                    </div>
                    {editMode === 'shared' && (
                      <DotEnvEditor value={sharedContent} onChange={v => { setSharedContent(v); setDirtyShared(true); }}
                        onSave={handleSaveShared} canSave={dirtyShared} saveLabel="Save shared"
                        hint="In git — shared with the team"
                        placeholder={`# .env.${selectedEnv}\nGRPC_ADDRESS=\n# KEY=VALUE`} />
                    )}
                    {editMode === 'local' && (
                      <>
                        {!hasLocalOverrides && <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 8 }}>No local overrides yet. Add values below.</div>}
                        <DotEnvEditor value={localContent} onChange={v => { setLocalContent(v); setDirtyLocal(true); }}
                          onSave={handleSaveLocal} canSave={dirtyLocal} saveLabel="Save local"
                          hint="Gitignored — only you see these"
                          placeholder="# Add KEY=VALUE lines to override shared values locally"
                          onDelete={hasLocalOverrides ? handleDeleteLocal : undefined} deleteLabel="Delete local overrides" />
                      </>
                    )}
                  </>
                )}
              </div>
            </div>
          )}

          {view.kind === 'list' && tab === 'browser' && (
            <div>
              <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 6, border: '1px solid var(--border)', borderRadius: 5, padding: '3px 6px', background: 'var(--bg-primary)' }}>
                <Search size={12} style={{ color: 'var(--text-muted)', flexShrink: 0 }} />
                <input value={envSearch} onChange={e => setEnvSearch(e.target.value)} placeholder="Filter environments…"
                  style={{ flex: 1, border: 'none', background: 'transparent', fontSize: 11, color: 'var(--text-primary)', outline: 'none' }} />
                {envSearch && <button onClick={() => setEnvSearch('')} style={{ ...btn('ghost', 'sm'), fontSize: 10, padding: '0 3px' }}>✕</button>}
              </div>

              {filteredEnvs.length === 0 && (
                <div style={{ fontSize: 12, color: 'var(--text-muted)', textAlign: 'center', padding: 20 }}>
                  {envSearch ? 'No matching environments' : 'No environments yet. Click "New" to create one.'}
                </div>
              )}

              {filteredEnvs.map(env => {
                const isActive = activeEnvironment === env.name;
                const varKeys = Object.keys(env.variables);
                const varCount = varKeys.length;
                const mutedCount = env.mutedVariables?.filter(k => k in env.variables).length || 0;
                const emptyCount = varKeys.filter(k => !env.variables[k]).length;

                return (
                  <div key={env.name} style={{
                    display: 'flex', alignItems: 'center', gap: 6, padding: '7px 8px',
                    borderRadius: 6, marginBottom: 3,
                    background: isActive ? `${colors.accent}08` : 'transparent',
                    border: isActive ? `1px solid ${colors.accent}25` : '1px solid transparent',
                    transition: 'background 0.1s',
                  }}
                    onMouseEnter={e => { if (!isActive) e.currentTarget.style.background = 'var(--bg-tertiary)'; }}
                    onMouseLeave={e => { if (!isActive) e.currentTarget.style.background = 'transparent'; }}
                  >
                    <button onClick={() => setActiveEnvironment(isActive ? null : env.name)} style={{
                      ...btn('ghost', 'sm'), flexShrink: 0, width: 24, height: 24, padding: 0,
                      color: isActive ? colors.accent : 'var(--text-muted)',
                    }} title={isActive ? 'Deactivate' : 'Activate'}>
                      {isActive ? <Check size={14} /> : <span style={{ width: 14, height: 14, borderRadius: '50%', border: '2px solid var(--border)', display: 'block' }} />}
                    </button>

                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{
                        fontSize: 12, fontWeight: isActive ? 600 : 400,
                        overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap',
                        color: isActive ? 'var(--accent)' : 'var(--text-primary)',
                      }}>
                        {env.name}
                        {isActive && <span style={{ fontSize: 9, color: colors.accent, marginLeft: 4 }}>active</span>}
                        {mutedCount > 0 && <span style={{ fontSize: 9, color: colors.warning, marginLeft: 4 }}>({mutedCount} muted)</span>}
                      </div>
                      <div style={{ fontSize: 10, color: 'var(--text-muted)', display: 'flex', gap: 6, alignItems: 'center' }}>
                        <span>{varCount} variable{varCount !== 1 ? 's' : ''}</span>
                        {emptyCount > 0 && <span style={{ color: `${colors.warning}99` }}>{emptyCount} empty (secrets)</span>}
                      </div>
                    </div>

                    <button onClick={() => openEdit(env)} style={btn('ghost', 'sm')} title="Edit"><Pencil size={13} /></button>
                    <button onClick={() => { addEnvironment({ ...env, name: `${env.name} (copy)` }); }} style={btn('ghost', 'sm')} title="Duplicate"><Copy size={13} /></button>
                    <button onClick={async () => { if (await modal.confirm('Delete', `Delete "${env.name}"?`)) deleteEnvironment(env.name); }} style={btn('ghost', 'sm')} title="Delete"><Trash2 size={13} /></button>
                  </div>
                );
              })}
            </div>
          )}
        </div>

        <div style={{ padding: '8px 12px', borderTop: '1px solid var(--border)', display: 'flex', gap: 6, flexShrink: 0 }}>
          {view.kind === 'list' && tab === 'browser' && <button onClick={openNew} style={btn('primary', 'sm')}><Plus size={12} /> New Environment</button>}
          {view.kind === 'list' && tab === 'project' && selectedEnv && (
            <span style={{ fontSize: 10, color: 'var(--text-muted)', alignSelf: 'center' }}>
              These are plain <code style={{ background: 'var(--bg-tertiary)', padding: '0 3px', borderRadius: 2, fontSize: 10 }}>.env</code> files — edit them in your editor too
            </span>
          )}
        </div>
      </div>
    </div>
  );
}
