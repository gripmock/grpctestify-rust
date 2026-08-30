import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { useShallow } from 'zustand/react/shallow';
import { looksLikeSecret } from '../../lib/secret-names';
import { environmentAddressNote, targetNote } from '../../lib/env-address';
import { useStore } from '../../lib/store';
import { useModal } from 'luvo/ui/ModalContext';
import { useToast } from 'luvo/ui/ToastContext';
import type { Environment, VariableUse } from '../../lib/types';
import { pathTail } from '../../lib/path-tail';
import { applyEntries, entriesOf, parseDotenv, serializeDotenv, type DotenvLine } from '../../lib/dotenv';
import { droppedNames, droppedQuestion } from '../../lib/env-drop';
import { blankRow, duplicateNames, filterNames, hiddenValue, missingNames, overriddenRow, putAddress, rankMissing, rowState, rowsOf, shouldKeepLocal, splitRows, takeAddress, valueNamesVariable, type Origin, type Row } from '../../lib/env-rows';
import { findVariables, unusableNames } from '../../lib/env';
import { Plus, Pencil, Trash2, Copy, Check, X, Globe, FolderGit2, Lock, LockOpen, Eye, EyeOff, ArrowLeft, FileText, Target, Search } from 'lucide-react';
import { count, plural } from 'luvo/data/plural';

interface Props {
  onClose: () => void;
  defineVar?: string | null;
  defineValue?: string;
}

function whereUsed(name: string, uses: VariableUse[], open: string[]): string {
  const use = uses.find(u => u.name === name);
  if (!use) return open.includes(name) ? 'the file on screen' : '';
  return use.count === 1 ? use.files[0] : `${use.files[0]} and ${use.count - 1} more`;
}

const MISSING_SHOWN = 6;

type View =
  | { kind: 'list' }
  | { kind: 'edit'; name: string; origin: Origin }
  | { kind: 'new' };

export function EnvironmentManager({ onClose, defineVar, defineValue }: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  useEffect(() => { dialogRef.current?.showModal(); }, []);

  const browserEnvs = useStore(s => s.browserEnvs);
  const projectEnvs = useStore(s => s.projectEnvs);
  const projectRoot = useStore(s => s.projectRoot);
  const activeEnvironment = useStore(s => s.activeEnvironment);
  const setActiveEnvironment = useStore(s => s.setActiveEnvironment);
  const addEnvironment = useStore(s => s.addEnvironment);
  const updateEnvironment = useStore(s => s.updateEnvironment);
  const deleteEnvironment = useStore(s => s.deleteEnvironment);
  const fetchProjectEnv = useStore(s => s.fetchProjectEnv);
  const saveProjectEnv = useStore(s => s.saveProjectEnv);
  const fetchProjectEnvLocal = useStore(s => s.fetchProjectEnvLocal);
  const saveProjectEnvLocal = useStore(s => s.saveProjectEnvLocal);
  const refreshProjectEnvs = useStore(s => s.refreshProjectEnvs);
  const deleteProjectEnv = useStore(s => s.deleteProjectEnv);
  const deleteProjectEnvLocal = useStore(s => s.deleteProjectEnvLocal);
  const fetchVariableUses = useStore(s => s.fetchVariableUses);

  const modal = useModal();
  const toast = useToast();

  const [view, setView] = useState<View>({ kind: 'list' });
  const [rows, setRows] = useState<Row[]>([blankRow()]);
  const [busy, setBusy] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [showValues, setShowValues] = useState(false);
  const [missingOpen, setMissingOpen] = useState(false);
  const [missingFilter, setMissingFilter] = useState('');
  const [name, setName] = useState('');
  const [place, setPlace] = useState<Origin>(projectRoot ? 'project' : 'browser');
  const [sharedLines, setSharedLines] = useState<DotenvLine[]>([]);
  const [localLines, setLocalLines] = useState<DotenvLine[]>([]);
  const [address, setAddress] = useState('');
  const [addressLocal, setAddressLocal] = useState(false);
  const [addressShared, setAddressShared] = useState<string | undefined>(undefined);
  const [tls, setTls] = useState<boolean | undefined>(undefined);
  const [tlsOpen, setTlsOpen] = useState(false);
  const [tlsCa, setTlsCa] = useState('');
  const [tlsCert, setTlsCert] = useState('');
  const [tlsKey, setTlsKey] = useState('');
  const [tlsInsecure, setTlsInsecure] = useState(true);

  const open = useStore(useShallow(s => {
    const text = [s.request.endpoint, ...Object.values(s.request.headers), ...s.request.bodies].join('\n');
    return findVariables(text);
  }));
  const [uses, setUses] = useState<VariableUse[]>([]);
  useEffect(() => { void fetchVariableUses().then(setUses).catch(() => setUses([])); }, [fetchVariableUses]);
  const needed = useMemo(
    () => [...new Set([...open, ...uses.map(u => u.name)])],
    [open, uses],
  );

  const all = useMemo(
    () => [...projectEnvs, ...browserEnvs.filter(b => !projectEnvs.some(p => p.name === b.name))],
    [projectEnvs, browserEnvs],
  );

  const openProject = useCallback(async (envName: string, extra?: string, extraValue = '') => {
    setBusy(true);
    try {
      const [shared, local] = await Promise.all([
        fetchProjectEnv(envName).catch(() => ''),
        fetchProjectEnvLocal(envName).catch(() => ({ exists: false, content: null })),
      ]);
      const sharedParsed = parseDotenv(typeof shared === 'string' ? shared : '');
      const localParsed = parseDotenv(local.content || '');
      setSharedLines(sharedParsed);
      setLocalLines(localParsed);
      const lifted = takeAddress(rowsOf(entriesOf(sharedParsed), entriesOf(localParsed)));
      const loaded = lifted.rows;
      setAddress(lifted.address);
      setAddressLocal(lifted.addressLocal);
      setAddressShared(lifted.addressShared);
      setRows([...loaded, ...(extra && !loaded.some(r => r.key === extra)
        ? [{ key: extra, value: extraValue, local: looksLikeSecret(extra) }]
        : []), blankRow()]);
      setName(envName);
      setTls(undefined);
      setView({ kind: 'edit', name: envName, origin: 'project' });
      setDirty(false);
    } finally {
      setBusy(false);
    }
  }, [fetchProjectEnv, fetchProjectEnvLocal]);

  const openBrowser = useCallback((env: Environment, extra?: string, extraValue = '') => {
    const loaded: Row[] = Object.entries(env.variables).map(([key, value]) => ({ key, value, local: false }));
    setAddress(env.address || '');
    setAddressLocal(false);
    setRows([...loaded, ...(extra && !loaded.some(r => r.key === extra) ? [{ key: extra, value: extraValue, local: false }] : []), blankRow()]);
    setName(env.name);
    setSharedLines([]); setLocalLines([]);
    setTls(env.tls);
    setTlsOpen(env.tls !== undefined);
    setTlsCa(env.tlsCa || ''); setTlsCert(env.tlsCert || ''); setTlsKey(env.tlsKey || '');
    setTlsInsecure(env.tlsInsecure ?? true);
    setView({ kind: 'edit', name: env.name, origin: 'browser' });
    setDirty(false);
  }, []);

  const prefilled = useRef(false);
  useEffect(() => {
    if (prefilled.current || !defineVar) return;
    prefilled.current = true;
    const active = all.find(e => e.name === activeEnvironment);
    if (!active) {
      setView({ kind: 'new' });
      setName('');
      setRows([{ key: defineVar, value: defineValue ?? '', local: looksLikeSecret(defineVar) }, blankRow()]);
      return;
    }
    if (active.source === 'project') void openProject(active.name, defineVar, defineValue);
    else openBrowser(active, defineVar, defineValue);
  }, [defineVar, defineValue, all, activeEnvironment, openProject, openBrowser]);

  const setRow = (i: number, patch: Partial<Row>) => {
    setRows(current => {
      const next = current.map((r, j) => {
        if (j !== i) return r;
        const merged = { ...r, ...patch };
        const origin: Origin = view.kind === 'edit' ? view.origin : 'browser';
        if (patch.value !== undefined && shouldKeepLocal(r, origin, patch.value)) {
          return { ...merged, local: true, shared: r.value };
        }
        return merged;
      });
      if (next[next.length - 1].key.trim() || next[next.length - 1].value.trim()) next.push(blankRow());
      return next;
    });
    setDirty(true);
  };

  const removeRow = (i: number) => {
    setRows(current => {
      const next = current.filter((_, j) => j !== i);
      return next.length > 0 ? next : [blankRow()];
    });
    setDirty(true);
  };

  const toggleLocal = (i: number) => {
    setRows(current => current.map((r, j) => {
      if (j !== i) return r;
      return r.local
        ? { ...r, local: false, shared: undefined }
        : { ...r, local: true, shared: r.shared ?? '' };
    }));
    setDirty(true);
  };

  const saveProject = async (envName: string) => {
    const { shared, local } = splitRows(putAddress(rows, address, addressLocal, addressShared));
    const before = [...entriesOf(sharedLines), ...entriesOf(localLines)].map(([key]) => key);
    const dropped = droppedNames(before, [...shared, ...local].map(([key]) => key), uses);
    if (dropped.length > 0) {
      const ok = await modal.confirm('Save environment', droppedQuestion(dropped), {
        confirmText: 'Save',
        danger: true,
      });
      if (!ok) return;
    }
    setBusy(true);
    try {
      await saveProjectEnv(envName, serializeDotenv(applyEntries(sharedLines, shared)));
      if (local.length > 0 || localLines.length > 0) {
        await saveProjectEnvLocal(envName, serializeDotenv(applyEntries(localLines, local)));
      }
      await refreshProjectEnvs();
      toast.success(`Saved .env.${envName}${local.length > 0 ? ` and .env.${envName}.local` : ''}`);
      setView({ kind: 'list' });
      setDirty(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Could not save the environment');
    } finally {
      setBusy(false);
    }
  };

  const saveBrowser = (original: string | null) => {
    const trimmed = name.trim();
    if (!trimmed) return;
    const variables = Object.fromEntries(rows.filter(r => r.key.trim()).map(r => [r.key.trim(), r.value]));
    const env: Environment = { name: trimmed, variables, source: 'browser' };
    if (address.trim()) env.address = address.trim();
    if (tls !== undefined) {
      env.tls = tls;
      if (tls) {
        if (tlsCa.trim()) env.tlsCa = tlsCa.trim();
        if (tlsCert.trim()) env.tlsCert = tlsCert.trim();
        if (tlsKey.trim()) env.tlsKey = tlsKey.trim();
        env.tlsInsecure = tlsInsecure;
      }
    }
    if (original) updateEnvironment(original, env);
    else addEnvironment(env);
    setView({ kind: 'list' });
    setDirty(false);
  };

  const create = async () => {
    const trimmed = name.trim();
    if (!trimmed) return;
    if (place === 'browser') {
      setAddress('');
      addEnvironment({ name: trimmed, variables: {}, source: 'browser' });
      openBrowser({ name: trimmed, variables: {}, source: 'browser' });
      return;
    }
    setBusy(true);
    try {
      await saveProjectEnv(trimmed, `# .env.${trimmed}\n`);
      useStore.setState(s => ({ projectEnvNames: [...new Set([...s.projectEnvNames, trimmed])].sort() }));
      await refreshProjectEnvs();
      await openProject(trimmed);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : 'Could not create the file');
    } finally {
      setBusy(false);
    }
  };

  const nameTaken = view.kind === 'new' && !!name.trim() && all.some(e => e.name === name.trim());
  const used = useMemo(() => new Set(needed), [needed]);
  const unusable = useMemo(() => unusableNames(rows.map(r => r.key)), [rows]);
  const repeated = useMemo(() => duplicateNames(rows), [rows]);
  const missing = useMemo(
    () => (view.kind === 'edit' ? rankMissing(missingNames(needed, rows), uses) : []),
    [view.kind, needed, rows, uses],
  );
  const shown = filterNames(missing, missingFilter);
  const drawn = missingOpen ? shown : shown.slice(0, MISSING_SHOWN);
  const addMissing = (names: string[] = missing) => {
    setRows(current => [...current.filter(r => r.key.trim() || r.value.trim()),
      ...names.map(key => ({ key, value: '', local: false })), blankRow()]);
    setDirty(true);
  };

  const discarded = async () =>
    !dirty || await modal.confirm('Discard', 'Discard changes to this environment?', { confirmText: 'Discard', danger: true });

  const leave = async () => {
    if (!await discarded()) return;
    setView({ kind: 'list' });
    setDirty(false);
  };

  const close = async () => {
    if (await discarded()) onClose();
  };

  return (
    <dialog
      ref={dialogRef}
      className="modal env-modal"
      aria-label="Environments"
      onCancel={e => { e.preventDefault(); void close(); }}
      onClose={() => { void close(); }}
      onClick={e => { if (e.target === dialogRef.current) void close(); }}
    >
      <div className="modal-head">
        <span className="bar">
          {view.kind !== 'list' && (
            <button className="btn is-ghost is-icon" onClick={() => void leave()} aria-label="Back"><ArrowLeft size={14} /></button>
          )}
          <h2 className="modal-title">
            {view.kind === 'list' ? 'Environments' : view.kind === 'new' ? 'New environment' : name}
          </h2>
          {view.kind === 'edit' && (
            <span className="badge">{view.origin === 'project' ? `.env.${name}` : 'this browser'}</span>
          )}
        </span>
        <button className="btn is-ghost is-icon" onClick={() => void close()} aria-label="Close"><X size={14} /></button>
      </div>

      <div className="modal-body stack">
        {view.kind === 'list' && (
          all.length === 0 ? (
            <div className="empty">
              An environment is a set of <span className="mono">{'{{KEY}}'}</span> values a call is sent with.
            </div>
          ) : (
            <div className="stack is-tight">
              {all.map(env => {
                const isActive = activeEnvironment === env.name;
                const count = Object.keys(env.variables).length;
                return (
                  <div key={env.name} className={`row env-row${isActive ? ' is-on' : ''}`}>
                    <button
                      className="env-pick"
                      onClick={() => setActiveEnvironment(isActive ? null : env.name)}
                      title={isActive ? 'Switch it off' : 'Use this environment'}
                    >
                      {isActive ? <Check size={13} /> : <span className="radio" />}
                      <span className="row-name">{env.name}</span>
                      <span className="badge is-kind">
                        {env.source === 'project' ? <FolderGit2 size={10} /> : <Globe size={10} />}
                        {env.source === 'project' ? 'file' : 'browser'}
                      </span>
                      <span className="muted">{count} {plural(count, 'variable')}</span>
                      {env.address && <span className="mono muted env-target-note">→ {env.address}</span>}
                    </button>

                    <button
                      className="btn is-ghost is-icon"
                      onClick={() => (env.source === 'project' ? void openProject(env.name) : openBrowser(env))}
                      title="Edit"
                    >
                      <Pencil size={13} />
                    </button>
                    {env.source === 'browser' && (
                      <button
                        className="btn is-ghost is-icon"
                        onClick={() => addEnvironment({ ...env, name: `${env.name} copy` })}
                        title="Duplicate"
                      >
                        <Copy size={13} />
                      </button>
                    )}
                    <button
                      className="btn is-ghost is-icon"
                      onClick={async () => {
                        const gone = droppedNames(Object.keys(env.variables), [], uses);
                        const what = [
                          env.source === 'project'
                            ? `Delete .env.${env.name} and its local overrides?`
                            : `Delete "${env.name}"?`,
                          gone.length > 0 ? droppedQuestion(gone) : null,
                          isActive ? 'It is the environment this project is set to, so calls fall back to what the header names.' : null,
                        ].filter(Boolean).join(' ');
                        if (!await modal.confirm('Delete', what, { confirmText: 'Delete', danger: true })) return;
                        if (env.source === 'project') {
                          try { await deleteProjectEnv(env.name); } catch (err) {
                            toast.error(err instanceof Error ? err.message : 'Could not delete the file');
                          }
                        } else {
                          deleteEnvironment(env.name);
                        }
                      }}
                      title={env.source === 'project' ? `Delete .env.${env.name}` : 'Delete'}
                    >
                      <Trash2 size={13} />
                    </button>
                  </div>
                );
              })}
            </div>
          )
        )}

        {view.kind === 'new' && (
          <div className="stack">
            <input
              className={`field mono${nameTaken ? ' is-bad' : ''}`}
              value={name}
              autoFocus
              onChange={e => setName(e.target.value)}
              onKeyDown={e => { if (e.key === 'Enter' && !nameTaken) void create(); }}
              placeholder="staging"
            />
            {nameTaken && <div className="field-error">There is already an environment called “{name.trim()}”.</div>}
            {projectRoot && (
              <Seg
                label="Where this environment lives"
                value={place}
                onChange={setPlace}
                options={[
                  { value: 'project', label: <><FileText size={11} /> a file in the project</> },
                  { value: 'browser', label: <><Globe size={11} /> this browser only</> },
                ]}
              />
            )}
            <span className="muted">
              {place === 'project'
                ? `Written as ${projectRoot}/.env.${name.trim() || '<name>'} — in git, shared with the team.`
                : 'Kept in this browser. Nothing is written to the project.'}
            </span>
            <div className="bar">
              <button className="btn is-primary" onClick={() => void create()} disabled={!name.trim() || nameTaken || busy}>
                <Check size={12} /> Create
              </button>
              <button className="btn is-quiet" onClick={() => setView({ kind: 'list' })}>Cancel</button>
            </div>
          </div>
        )}

        {view.kind === 'edit' && (
          <div
            className="stack"
            onKeyDown={e => {
              if (e.key !== 'Enter' || e.shiftKey || busy) return;
              if (!(e.target instanceof HTMLInputElement)) return;
              e.preventDefault();
              if (view.origin === 'project') void saveProject(view.name);
              else saveBrowser(view.name);
            }}
          >
            {view.origin === 'browser' && (
              <input className="field mono" value={name} onChange={e => { setName(e.target.value); setDirty(true); }} placeholder="name" />
            )}

            <div className="env-target">
              <span className="label"><Target size={11} /> target</span>
              <span />
              <input
                className="field mono"
                value={address}
                autoFocus
                onChange={e => { setAddress(e.target.value); setDirty(true); }}
                placeholder="http://host:port — empty means the address in the header"
                spellCheck={false}
              />
              <span className="var-tools">
                {view.origin === 'project' && address.trim() !== '' && (
                  <button
                    className={`btn is-ghost is-icon${addressLocal ? ' is-warned' : ''}`}
                    onClick={() => { setAddressLocal(v => !v); setDirty(true); }}
                    title={addressLocal
                      ? `Kept in .env.${name}.local — this machine dials it; the shared file keeps ${addressShared ? `the team's ${addressShared}` : 'the name and no value'}`
                      : 'Keep this target out of git'}
                  >
                    {addressLocal ? <Lock size={12} /> : <LockOpen size={12} />}
                  </button>
                )}
              </span>
            </div>

            {targetNote(address) && (
              <div className="note env-target-scheme">{targetNote(address)}</div>
            )}

            {(() => {
              const said = environmentAddressNote(address);
              return said === null ? null : (
                <div className={`note${said.bad ? ' is-warn' : ''} env-target-graded`}>{said.said}</div>
              );
            })()}

            {missing.length > 0 && (
              <div className="env-missing stack is-tight">
                <div className="bar">
                  <span className="label grow">
                    {missing.length === 1 ? 'one name the files ask for' : `${missing.length} names the files ask for`}, not set here
                  </span>
                  {missing.length <= MISSING_SHOWN * 2 && (
                    <button className="btn is-sm" onClick={() => addMissing()}>
                      <Plus size={11} /> add {missing.length === 1 ? 'it' : `all ${missing.length}`}
                    </button>
                  )}
                </div>

                {missing.length > MISSING_SHOWN && (
                  <div className="field-frame env-missing-filter">
                    <Search size={11} className="muted" />
                    <input
                      className="field"
                      value={missingFilter}
                      onChange={e => { setMissingFilter(e.target.value); setMissingOpen(true); }}
                      placeholder={`Filter ${count(missing.length, 'name')}…`}
                      spellCheck={false}
                    />
                    {missingFilter && (
                      <button className="btn is-ghost is-icon" onClick={() => setMissingFilter('')} aria-label="Clear filter">
                        <X size={11} />
                      </button>
                    )}
                  </div>
                )}

                <div className="env-missing-list">
                  {drawn.map(name => (
                    <button key={name} className="row env-missing-row" onClick={() => addMissing([name])}>
                      <span className="mono row-name">{'{{'}{name}{'}}'}</span>
                      <span className="muted grow env-missing-where">{whereUsed(name, uses, open)}</span>
                      <Plus size={11} />
                    </button>
                  ))}
                  {shown.length === 0 && <div className="muted env-missing-none">No name matches “{missingFilter}”.</div>}
                </div>

                {shown.length > MISSING_SHOWN && (
                  <button className="btn is-quiet is-sm" onClick={() => setMissingOpen(v => !v)}>
                    {missingOpen ? 'show fewer' : `show all ${shown.length}`}
                  </button>
                )}
              </div>
            )}

            {repeated.length > 0 && (
              <div className="note is-warn env-repeated">
                {repeated.length === 1
                  ? `${repeated[0]} is written more than once`
                  : `${repeated.join(', ')} are each written more than once`}
                {' '}— a call reads the last row with the name, and the ones above it are not saved
                as anything.
              </div>
            )}

            <div className="var-table">
              <div className="var-head muted">
                <span>variable</span>
                <span>value</span>
                <span />
              </div>
              {rows.map((row, i) => (
                <div key={i} className={`var-line${row.local ? ' is-local' : ''}`}>
                  <input
                    className={`field mono${unusable.includes(row.key.trim()) || overriddenRow(rows, i) ? ' is-bad' : ''}`}
                    value={row.key}
                    onChange={e => setRow(i, { key: e.target.value })}
                    placeholder="KEY"
                    spellCheck={false}
                    title={unusable.includes(row.key.trim())
                      ? `A file cannot use {{${row.key.trim()}}} — a name starts with a letter or _ and holds letters, digits, _ and .`
                      : overriddenRow(rows, i)
                        ? `${row.key.trim()} is written again below — the last one is the value every call reads, and this row is not`
                        : undefined}
                  />
                  <input
                    className={`field mono${valueNamesVariable(row.value).length > 0 ? ' is-bad' : ''}`}
                    type={hiddenValue(row) && !showValues ? 'password' : 'text'}
                    value={row.value}
                    onChange={e => setRow(i, { value: e.target.value })}
                    placeholder={row.local && row.shared !== undefined ? row.shared : 'value'}
                    autoComplete="off"
                    spellCheck={false}
                    title={valueNamesVariable(row.value).length > 0
                      ? `A value is not read again: ${valueNamesVariable(row.value)
                          .map(n => `{{${n}}}`)
                          .join(', ')} is sent as written, braces and all`
                      : undefined}
                  />
                  <span className="var-tools">
                    {rowState(row) === 'empty' && (
                      <span
                        className="badge is-pending"
                        title={`${row.key.trim()} has no value — every {{${row.key.trim()}}} is sent as an empty string`}
                      >
                        no value
                      </span>
                    )}
                    {rowState(row) === 'awaiting-local' && (
                      <span
                        className="badge is-pending"
                        title={`.env.${name} names ${row.key.trim()} and leaves the value to each machine — this one has none, so the call sends an empty string. Typing it here keeps it in .env.${name}.local, out of git.`}
                      >
                        not on this machine
                      </span>
                    )}
                    {used.has(row.key.trim()) && (
                      <span className="badge" title="A file in this workbench asks for this variable">used</span>
                    )}
                    {view.origin === 'project' && row.key.trim() && row.value.trim() !== '' && !row.local && looksLikeSecret(row.key) && (
                      <span className="warn env-secret" title={`${row.key.trim()} reads as a credential, and .env.${name} is shared — lock it to keep the value on this machine`}>
                        shared
                      </span>
                    )}
                    {view.origin === 'project' && row.key.trim() && (
                      <button
                        className={`btn is-ghost is-icon${row.local ? ' is-warned' : ''}${!row.local && row.value.trim() !== '' && looksLikeSecret(row.key) ? ' is-urged' : ''}`}
                        onClick={() => toggleLocal(i)}
                        title={row.local
                          ? `Kept in .env.${name}.local — gitignored, only on this machine`
                          : looksLikeSecret(row.key)
                            ? `${row.key.trim()} reads as a credential — keep it out of git, in .env.${name}.local`
                            : `Keep this value out of git — moves it to .env.${name}.local`}
                      >
                        {row.local ? <Lock size={12} /> : <LockOpen size={12} />}
                      </button>
                    )}
                    {(row.key.trim() || row.value.trim()) && (
                      <button className="btn is-ghost is-icon" onClick={() => removeRow(i)} title="Remove"><X size={11} /></button>
                    )}
                  </span>
                </div>
              ))}
            </div>

            {unusable.length > 0 && (
              <div className="note is-warn">
                <span className="mono">{unusable.join(', ')}</span>{' '}
                {unusable.length === 1 ? 'is not a name a file can use' : 'are not names a file can use'} —{' '}
                <span className="mono">{`{{${unusable[0]}}}`}</span> stays as written. A name starts with a
                letter or <span className="mono">_</span> and holds letters, digits,{' '}
                <span className="mono">_</span> and <span className="mono">.</span>
              </div>
            )}

            {rows.some(hiddenValue) && (
              <div className="bar">
                <button className="btn is-ghost is-sm" onClick={() => setShowValues(v => !v)}>
                  {showValues ? <EyeOff size={11} /> : <Eye size={11} />} {showValues ? 'Hide' : 'Show'} hidden values
                </button>
                <span className="muted">{rows.filter(r => r.local).length} kept out of git</span>
                <span className="grow" />
                {view.origin === 'project' && localLines.length > 0 && (
                  <button
                    className="btn is-ghost is-sm is-danger"
                    disabled={busy}
                    onClick={async () => {
                      const ok = await modal.confirm(
                        `Forget the values in .env.${name}.local?`,
                        rows.filter(r => r.local).length === 1
                          ? `One value lives only on this machine and is not in git — this cannot be undone. The shared .env.${name} keeps its name.`
                          : `${rows.filter(r => r.local).length} values live only on this machine and are not in git — this cannot be undone. The shared .env.${name} keeps their names.`,
                        { confirmText: 'forget', danger: true },
                      );
                      if (!ok) return;
                      setBusy(true);
                      try {
                        await deleteProjectEnvLocal(name);
                        await refreshProjectEnvs();
                        await openProject(name);
                        toast.success(`.env.${name}.local is gone — the names are still in .env.${name}`);
                      } catch (err) {
                        toast.error(err instanceof Error ? err.message : 'Could not delete the file');
                      } finally {
                        setBusy(false);
                      }
                    }}
                  >
                    <Trash2 size={11} /> forget machine-only values
                  </button>
                )}
              </div>
            )}

            {view.origin === 'browser' && (
              <div className="stack is-tight">
                {!tlsOpen ? (
                  <button className="btn is-quiet is-sm" onClick={() => setTlsOpen(true)}>
                    <Lock size={11} /> Connection — {tls === undefined ? 'follows the header' : tls ? 'TLS' : 'plaintext'}
                  </button>
                ) : (
                  <div className="editor-frame stack is-tight">
                    <div className="label">Connection</div>
                    <Seg
                      label="Transport security for this environment"
                      value={tls === undefined ? 'global' : tls ? 'on' : 'off'}
                      onChange={key => { setTls(key === 'global' ? undefined : key === 'on'); setDirty(true); }}
                      options={[
                        { value: 'global', label: 'follow the header' },
                        { value: 'on', label: 'TLS' },
                        { value: 'off', label: 'plaintext' },
                      ]}
                    />
                    {tls && (
                      <div className="stack is-tight">
                        {([
                          ['CA certificate path', tlsCa, setTlsCa],
                          ['Client certificate path', tlsCert, setTlsCert],
                          ['Client key path', tlsKey, setTlsKey],
                        ] as const).map(([label, value, setter]) => (
                          <input key={label} className="field mono" value={value} placeholder={label}
                            onChange={e => { setter(e.target.value); setDirty(true); }} />
                        ))}
                        <label className="bar muted">
                          <input type="checkbox" checked={tlsInsecure} onChange={e => { setTlsInsecure(e.target.checked); setDirty(true); }} />
                          Skip certificate verification (insecure)
                        </label>
                      </div>
                    )}
                  </div>
                )}
              </div>
            )}

          </div>
        )}
      </div>

      {view.kind === 'edit' && (
        <div className="modal-foot env-foot">
          <span
            className="muted grow env-foot-note"
            title={view.origin === 'project'
              ? `${projectRoot}/.env.${name}${rows.some(r => r.local) || addressLocal ? `\n${projectRoot}/.env.${name}.local` : ''}`
              : undefined}
          >
            {view.origin === 'project'
              ? `Writes ${pathTail(`${projectRoot}/.env.${name}`)}${rows.some(r => r.local) || addressLocal ? ` and .env.${name}.local` : ''}`
              : 'Kept in this browser'}
          </span>
          <button className="btn is-quiet" onClick={() => void leave()}>Cancel</button>
          <button
            className="btn is-primary"
            disabled={busy || unusable.length > 0 || (view.origin === 'browser' && !name.trim())}
            title={unusable.length > 0
              ? `${unusable.join(', ')} — a file cannot use ${unusable.length === 1 ? 'that name' : 'those names'}, so it would be written and never read`
              : undefined}
            onClick={() => (view.origin === 'project' ? void saveProject(view.name) : saveBrowser(view.name))}
          >
            <Check size={12} /> Save
          </button>
        </div>
      )}

      {view.kind === 'list' && (
        <div className="modal-foot">
          <button
            className="btn is-primary is-sm"
            onClick={() => { setView({ kind: 'new' }); setName(''); setRows([blankRow()]); setPlace(projectRoot ? 'project' : 'browser'); }}
          >
            <Plus size={12} /> New environment
          </button>
          <span className="muted">
            {projectRoot ? 'Files live in the project and are shared; browser environments stay here.' : 'Environments are kept in this browser.'}
          </span>
        </div>
      )}
    </dialog>
  );
}
