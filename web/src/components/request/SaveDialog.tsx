import { useEffect, useMemo, useRef, useState } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { apiPath } from '../../lib/api-path';
import { callAddress, rawIsAuthoritative, useStore } from '../../lib/store';
import { buildTree, childFolders, crumbs, familyOf, fileCount, saveFolders, sortTree } from '../../lib/tree';
import { isHttpRequest, looksHttp, suggestedFileName } from '../../lib/http-endpoint';
import { lineDiff, hasChanges } from 'luvo/data/diff';
import { Diff } from 'luvo/ui/Diff';
import type { GctfDiagnostic, GctfMeta, TreeNode } from '../../lib/types';
import { useDebouncedPost } from 'luvo/data/useDebouncedPost';
import { blocksSave, countBySeverity, problemCensus, sortProblems } from '../../lib/problems';
import { ChevronRight, Folder, FolderPlus, Tag, X } from 'lucide-react';
import { useModal } from 'luvo/ui/ModalContext';
import { addressForSave, metaFromParsed } from '../../lib/save-meta';
import { readText, writeText } from 'luvo/data/storage';
import { count } from 'luvo/data/plural';
import { seedSaveName } from '../../lib/save-name';

export function SaveDialog({
  onClose,
  onSave,
}: {
  onClose: () => void;
  onSave: (path: string, meta: GctfMeta, fmt: boolean) => Promise<void>;
}) {
  const collections = useStore(s => s.collections);
  const previewSave = useStore(s => s.previewSave);
  const parsed = useStore(s => s.collectionParsed);
  const endpoint = useStore(s => s.request.endpoint);
  const workspacePath = useStore(s => s.workspacePath);
  const protocol = useStore(s => s.protocol);
  const documents = useStore(s => s.documents);
  const mixed = useMemo(() => {
    const kinds = new Set(documents.map(d => (looksHttp(d.endpoint) ? 'http' : 'grpc')));
    return documents.length > 1 && kinds.size > 1;
  }, [documents]);
  const [ext, setExt] = useState<'gctf' | 'httf' | 'apif'>(() => {
    const st = useStore.getState();
    const steps = new Set(st.documents.map(d => (looksHttp(d.endpoint) ? 'http' : 'grpc')));
    if (st.documents.length > 1 && steps.size > 1) return 'apif';
    return isHttpRequest(workspacePath, st.request.endpoint) ? 'httf' : 'gctf';
  });
  const address = useStore(s => s.address);
  const addressTouched = useStore(s => s.addressTouched);
  const rawWins = useStore(s => rawIsAuthoritative(s));

  const familyNote = mixed && ext !== 'apif'
    ? `This chain has steps of both kinds — only a .apif holds them; a .${ext} runs one transport`
    : ext === 'apif' || endpoint.trim() === '' || ext === (looksHttp(endpoint) ? 'httf' : 'gctf')
      ? null
      : ext === 'gctf'
        ? `${endpoint.trim()} is a method and a path — a .gctf calls a service and a method`
        : `${endpoint.trim()} is a service and a method — a .httf calls a method and a path`;

  const ref = useRef<HTMLDialogElement>(null);
  const [folder, setFolder] = useState(() =>
    workspacePath ? workspacePath.split('/').slice(0, -1).join('/') : '');
  const seeded = useMemo(() => {
    if (workspacePath) {
      return { name: workspacePath.split('/').pop()!.replace(/\.(gctf|httf|apif)$/, ''), taken: null };
    }
    const st = useStore.getState();
    return seedSaveName({
      base: suggestedFileName(endpoint) || 'untitled',
      ext,
      folder: '',
      paths: st.collections.filter(c => !c.is_dir).map(c => c.path),
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const [name, setName] = useState(() => seeded.name);
  const [runFmt, setRunFmt] = useState(() => readText('play.save.fmt', 'on') !== 'off');
  const [meta, setMeta] = useState<GctfMeta>(() => metaFromParsed(parsed));
  const [showMeta, setShowMeta] = useState(() => {
    const m = metaFromParsed(parsed);
    return !!(m.name || m.summary || m.owner || (m.tags?.length ?? 0) > 0 || (m.links?.length ?? 0) > 0);
  });
  const [tagDraft, setTagDraft] = useState('');
  const [linkDraft, setLinkDraft] = useState('');
  const [preview, setPreview] = useState<{ content: string; current: string | null } | null>(null);
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [creating, setCreating] = useState(false);
  const refreshCollections = useStore(s => s.refreshCollections);
  const modal = useModal();

  const makeFolder = async () => {
    const name = await modal.prompt('New folder', `Inside ${folder || 'the project root'}`);
    if (!name?.trim()) return;
    const full = folder ? `${folder}/${name.trim()}` : name.trim();
    setCreating(true);
    try {
      const res = await fetch(`/api/dir/${apiPath(full)}`, { method: 'POST' });
      if (res.ok) {
        await refreshCollections();
        setFolder(full);
      }
    } finally {
      setCreating(false);
    }
  };

  const carried = useMemo(() => {
    const out: { what: string; as: string }[] = [];
    if (protocol && protocol !== 'grpc') out.push({ what: protocol, as: 'OPTIONS.protocol' });
    const saved = addressForSave(parsed, address, addressTouched);
    if (saved) out.push({ what: saved, as: 'ADDRESS' });
    return out;
  }, [protocol, address, parsed, addressTouched]);

  const carriesAddress = carried.some(c => c.as === 'ADDRESS');
  const aimedAt = useStore(callAddress);
  const setAddress = useStore(s => s.setAddress);

  const [folderQuery, setFolderQuery] = useState('');
  const allDirs = useMemo(() => sortTree(buildTree(collections)).flatMap(collectDirs), [collections]);
  const inside = useMemo(() => childFolders(allDirs, folder), [allDirs, folder]);
  const matches = useMemo(
    () => (folderQuery.trim() === '' ? [] : saveFolders(allDirs, folderQuery)),
    [allDirs, folderQuery]);
  const searching = folderQuery.trim() !== '';
  const trail = useMemo(() => crumbs(folder), [folder]);
  const here = useMemo(() => allDirs.find(d => d.path === folder) ?? null, [allDirs, folder]);
  const hereFiles = here ? fileCount(here) : collections.filter(c => !c.is_dir && !c.path.includes('/')).length;

  useEffect(() => {
    ref.current?.showModal();
  }, []);

  const fileName = `${name.trim()}.${ext}`;
  const path = folder ? `${folder}/${fileName}` : fileName;
  const named = name.trim().length > 0;

  useEffect(() => {
    if (!named) return;
    let stale = false;
    const t = setTimeout(async () => {
      const p = await previewSave(path, meta, runFmt);
      if (stale) return;
      if (p.error !== undefined) { setPreview(null); setPreviewError(p.error); return; }
      setPreviewError(null);
      setPreview({ content: p.content, current: p.current });
    }, 150);
    return () => { stale = true; clearTimeout(t); };
  }, [path, meta, named, runFmt, previewSave, address, addressTouched, protocol]);

  const { data: found } = useDebouncedPost<GctfDiagnostic[]>(
    '/api/diagnostics',
    preview?.content ? { content: preview.content, file_name: path } : null,
    200,
  );
  const problems = useMemo(() => sortProblems(found ?? []), [found]);
  const counts = countBySeverity(problems);
  const broken = blocksSave(problems);

  const overwrite = named && preview?.current != null;
  const diff = overwrite ? lineDiff(preview!.current!, preview!.content) : null;
  const identical = diff != null && !hasChanges(diff);
  const stat = diff ? diffStat(diff) : null;

  const addLink = () => {
    const link = linkDraft.trim();
    if (!link) return;
    if (!(meta.links ?? []).includes(link)) setMeta({ ...meta, links: [...(meta.links ?? []), link] });
    setLinkDraft('');
  };

  const addTag = () => {
    const t = tagDraft.trim();
    if (!t) return;
    if (!(meta.tags ?? []).includes(t)) setMeta({ ...meta, tags: [...(meta.tags ?? []), t] });
    setTagDraft('');
  };

  const submit = async () => {
    if (!named || saving) return;
    setSaving(true);
    try {
      writeText('play.save.fmt', runFmt ? 'on' : 'off');
      await onSave(path, meta, runFmt);
      onClose();
    } finally {
      setSaving(false);
    }
  };

  return (
    <dialog
      ref={ref}
      className="modal is-lg"
      aria-label="Save file"
      onCancel={e => { e.preventDefault(); onClose(); }}
      onClose={() => onClose()}
      onClick={e => { if (e.target === ref.current) onClose(); }}
    >
      <div className="modal-head">
        <h2 className="modal-title">Save file</h2>
      </div>

      <div className="modal-body save-grid">
        <div className="stack save-col">
          <div>
            <div className="label">Folder</div>
            <input
              className="field save-folder-search"
              value={folderQuery}
              onChange={e => setFolderQuery(e.target.value)}
              placeholder="Search every folder…"
              spellCheck={false}
            />
            {!searching && (
              <div className="bar save-crumbs mono">
                {trail.map((crumb, i) => (
                  <span key={crumb.path || '/'} className="save-crumb">
                    {i > 0 && <span className="muted">/</span>}
                    <button
                      className={`btn is-ghost is-sm${i === trail.length - 1 ? ' is-on' : ''}`}
                      onClick={() => setFolder(crumb.path)}
                    >
                      {crumb.name}
                    </button>
                  </span>
                ))}
              </div>
            )}
            <div className="folder-list">
              {searching
                ? (matches.length === 0
                    ? <div className="empty">No folder matches</div>
                    : matches.map(f => (
                      <button
                        key={f.path}
                        className={`row save-folder${folder === f.path ? ' is-on' : ''}`}
                        onClick={() => { setFolder(f.path); setFolderQuery(''); }}
                      >
                        <Folder size={12} />
                        <span className="row-name mono">{f.path}</span>
                        <span className="row-note">{count(fileCount(f), 'file')}</span>
                      </button>
                    )))
                : (inside.length === 0
                    ? <div className="empty">{hereFiles > 0 ? `${count(hereFiles, 'file')} here, no folders` : 'Nothing inside yet'}</div>
                    : inside.map(f => (
                      <button
                        key={f.path}
                        className="row save-folder"
                        onClick={() => setFolder(f.path)}
                        title={`Save into ${f.path}`}
                      >
                        <Folder size={12} />
                        <span className="row-name">{f.name}</span>
                        <span className="row-note">{count(fileCount(f), 'file')}</span>
                        <ChevronRight size={11} className="muted" />
                      </button>
                    )))}
            </div>
            <button className="btn is-sm is-ghost" onClick={makeFolder} disabled={creating}>
              <FolderPlus size={11} /> new folder
            </button>
          </div>

          <div>
            <div className="label">File name</div>
            <div className="field-frame">
              <input
                className="field mono"
                value={name}
                onChange={e => {
                  const typed = e.target.value;
                  const named = familyOf(typed);
                  if (named === 'gctf' || named === 'httf' || named === 'apif') setExt(named);
                  setName(typed.replace(/\.(gctf|httf|apif)$/, ''));
                }}
                onKeyDown={e => { if (e.key === 'Enter') submit(); }}
                placeholder="login"
                autoFocus
              />
              <Seg
                className="save-family"
                label="Which family this file belongs to"
                value={ext}
                onChange={setExt}
                options={[
                  { value: 'gctf', label: '.gctf', title: 'gRPC — a service and a method' },
                  { value: 'httf', label: '.httf', title: 'HTTP — a method and a path' },
                  { value: 'apif', label: '.apif', title: 'Both — each step says which, by its own endpoint' },
                ]}
              />
            </div>
            <div className="muted mono save-path">{path || '—'}</div>
            {seeded.taken !== null && name === seeded.name && (
              <div className="muted save-taken">
                <span className="mono">{seeded.taken}</span> already exists — this saves beside it.
                Type that name to replace it instead.
              </div>
            )}
            {familyNote && <div className="note is-warn save-family-note">{familyNote}</div>}
          </div>

          {rawWins && (
            <div className="note">
              The source tab is what will be written, as it stands. Edit its META section there.
            </div>
          )}
          {!rawWins && !showMeta && (
            <button className="btn is-ghost is-sm save-meta-open" onClick={() => setShowMeta(true)}>
              <Tag size={11} /> add name, tags, owner…
            </button>
          )}
          {!rawWins && showMeta && (
          <fieldset className="panel">
            <legend>meta</legend>
            <div className="panel-body stack">
              <input className="field field-frame" placeholder="Name — how a report calls it"
                value={meta.name ?? ''} onChange={e => setMeta({ ...meta, name: e.target.value })} />
              <input className="field field-frame" placeholder="Summary"
                value={meta.summary ?? ''} onChange={e => setMeta({ ...meta, summary: e.target.value })} />
              <input className="field field-frame" placeholder="Owner"
                value={meta.owner ?? ''} onChange={e => setMeta({ ...meta, owner: e.target.value })} />
              <div>
                <div className="bar wrap">
                  {(meta.tags ?? []).map(t => (
                    <span key={t} className="chip is-on">
                      {t}
                      <button className="btn is-ghost is-icon" aria-label={`Remove ${t}`}
                        onClick={() => setMeta({ ...meta, tags: (meta.tags ?? []).filter(x => x !== t) })}>
                        <X size={9} />
                      </button>
                    </span>
                  ))}
                </div>
                <input
                  className="field field-frame"
                  placeholder="Tag, then Enter — the same names --tags selects in CI"
                  value={tagDraft}
                  onChange={e => setTagDraft(e.target.value)}
                  onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); addTag(); } }}
                  onBlur={addTag}
                />
              </div>

              <div>
                <div className="bar wrap">
                  {(meta.links ?? []).map(l => (
                    <span key={l} className="chip is-on mono">
                      {l}
                      <button className="btn is-ghost is-icon" aria-label={`Remove ${l}`}
                        onClick={() => setMeta({ ...meta, links: (meta.links ?? []).filter(x => x !== l) })}>
                        <X size={9} />
                      </button>
                    </span>
                  ))}
                </div>
                <input
                  className="field field-frame"
                  placeholder="Link — a doc or a ticket, then Enter"
                  value={linkDraft}
                  onChange={e => setLinkDraft(e.target.value)}
                  onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); addLink(); } }}
                  onBlur={addLink}
                />
              </div>
            </div>
          </fieldset>
          )}
        </div>

        <div className="stack save-col">
          <div className="bar">
            <span className="label grow">{overwrite ? 'Changes to the file on disk' : 'What will be written'}</span>
            {stat && stat.added > 0 && <span className="badge is-ok">+{stat.added}</span>}
            {stat && stat.removed > 0 && <span className="badge is-fail">−{stat.removed}</span>}
            {overwrite && !identical && <span className="badge is-fail">overwrite</span>}
            {identical && <span className="badge">no change</span>}
          </div>
          {problems.length > 0 && (
            <div className={`note save-problems${broken ? ' is-bad' : ''}`}>
              <span className="label">{problemCensus(counts)}</span>
              <span className="save-problem-first">{problems[0].message}</span>
              {problems.length > 1 && <span className="muted">+{problems.length - 1} more</span>}
            </div>
          )}

          {diff
            ? <Diff lines={diff} className="save-preview" />
            : <pre className="diff save-preview">{previewError ?? preview?.content ?? 'Reading what would be written…'}</pre>}
        </div>
      </div>

      {!rawWins && carried.length > 0 && (
        <div className="note save-note">
          {carried.map(c => (
            <div key={c.what}>
              {c.what} is a client setting — it will be written as <span className="mono">{c.as}</span>.
            </div>
          ))}
        </div>
      )}

      {!rawWins && !carriesAddress && aimedAt && (
        <div className="note save-note">
          <div>
            This file names no address — a run takes it from the environment or{' '}
            <span className="mono">$GRPCTESTIFY_ADDRESS</span>.
            <button className="btn is-sm is-ghost" onClick={() => setAddress(aimedAt)}>
              write {aimedAt} into it
            </button>
          </div>
        </div>
      )}

      <div className="modal-foot">
        <label className="bar grow save-fmt">
          <input type="checkbox" checked={runFmt} onChange={e => setRunFmt(e.target.checked)} />
          run <span className="mono">fmt</span> on save
        </label>
        <button className="btn is-quiet" onClick={onClose}>Cancel</button>
        <button
          className={`btn is-primary${overwrite && !identical ? ' is-danger' : ''}`}
          onClick={submit}
          disabled={!named || saving || previewError !== null}
          title={
            previewError ? `${previewError} — the save would be refused for the same reason`
            : broken ? 'The file would not pass `grpctestify check` — saving it is still allowed'
            : identical ? 'The file already says this — saving writes the same bytes'
            : undefined
          }
        >
          {saving ? 'Saving…' : broken ? 'Save anyway' : overwrite && !identical ? 'Overwrite' : 'Save'}
        </button>
      </div>
    </dialog>
  );
}

function diffStat(diff: ReturnType<typeof lineDiff>) {
  return {
    added: diff.filter(l => l.kind === 'add').length,
    removed: diff.filter(l => l.kind === 'del').length,
  };
}

function collectDirs(node: TreeNode): TreeNode[] {
  if (!node.isDir) return [];
  return [node, ...node.children.flatMap(collectDirs)];
}
