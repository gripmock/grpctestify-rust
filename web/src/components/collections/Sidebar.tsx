import { useMemo, useState, useCallback, useEffect } from 'react';
import { checkMark, rollUpChecks } from '../../lib/checked';
import { apiPath } from '../../lib/api-path';
import { RunSummary } from './RunBar';
import { failureLine, moreRowsNote, rollUp, slowNote, verdictLabel } from '../../lib/jobs';
import { deleteQuestion, deleteScope, referencedNote, renameBreaksNote, unsavedNote } from '../../lib/delete-warning';
import { durationLabel } from '../../lib/format';
import type { Verdict } from '../../lib/jobs';
import { moveRowFocus, rowIsTabStop, treeStep } from '../../lib/tree-keys';
import { isTabDirty, openRefusal, useStore } from '../../lib/store';
import { copiedNote } from '../../lib/duplicate-name';
import { unresolvedNames } from '../../lib/failure';
import { useModal } from 'luvo/ui/useModal';
import { useToast } from 'luvo/ui/useToast';
import type { TreeNode } from '../../lib/types';
import { copyToClipboard } from 'luvo/data/clipboard';
import { useDismiss } from 'luvo/input/useDismiss';
import { Popover } from 'luvo/ui/Popover';
import { ContextMenu } from 'luvo/ui/ContextMenu';
import { FileJson, Folder, FolderOpen, ChevronRight, RefreshCw, Search, X, Pencil, Trash2, FolderPlus, FilePlus, Copy, CopyPlus, MoreHorizontal, Tags, Globe, Network, Play, ShieldAlert, ShieldCheck } from 'lucide-react';

import { activeFilters, buildTree, collectTags, fileCount, filterByVerdict, filterTree, fixtureRole, railSaysWhy, reasonsSaidOnce, renameNote, runTargets, sortTree, familyOf, withFamilyExt } from '../../lib/tree';
import { newFileContent } from '../../lib/new-file';
import type { TagFilter } from '../../lib/tree';
import { count } from 'luvo/data/plural';
import { createRefusal, moveRefusal } from '../../lib/move-target';
import { NO_TOGGLES, expandedDirs, toggleDir, type DirToggles } from '../../lib/rail-expansion';

interface CtxMenu {
  x: number;
  y: number;
  node: TreeNode;
}

export function Sidebar() {
  const collections = useStore(s => s.collections);
  const verdicts = useStore(s => s.run.verdicts);
  const runFilter = useStore(s => s.runFilter);
  const runReason = useStore(s => s.runReason);
  const loadCollection = useStore(s => s.loadCollection);
  const selected = useStore(s => s.selectedCollection);
  const refreshCollections = useStore(s => s.refreshCollections);
  const read = useStore(s => s.collectionsRead);
  const workspaceName = useStore(s => s.workspaceName);
  const [toggles, setToggles] = useState<DirToggles>(NO_TOGGLES);
  const [search, setSearch] = useState('');
  const [tagFilter, setTagFilter] = useState<TagFilter>(() => ({ include: new Set(), exclude: new Set() }));
  const changedPaths = useStore(s => s.changedPaths);
  const [onlyChanged, setOnlyChanged] = useState(false);
  const checked = useStore(s => s.checked);
  const [onlyProblems, setOnlyProblems] = useState(false);
  const [ctxMenu, setCtxMenu] = useState<CtxMenu | null>(null);
  const modal = useModal();
  const toast = useToast();

  const allTags = useMemo(() => collectTags(collections), [collections]);

  const tree = useMemo(() => {
    const changedOnly = onlyChanged && changedPaths
      ? collections.filter(c => c.is_dir || changedPaths.includes(c.path))
      : collections;
    const source = onlyProblems
      ? changedOnly.filter(c => c.is_dir || checked[c.path] !== undefined)
      : changedOnly;
    const sorted = sortTree(buildTree(source));
    const filtering = search || tagFilter.include.size > 0 || tagFilter.exclude.size > 0
      || onlyChanged || onlyProblems;
    const filtered = filtering ? filterTree(sorted, search, tagFilter) : sorted;
    return filterByVerdict(filtered, verdicts, runFilter, runReason);
  }, [collections, search, tagFilter, verdicts, runFilter, runReason, onlyChanged, changedPaths, onlyProblems, checked]);

  const rootDirs = useMemo(
    () => tree.filter(n => n.isDir && fileCount(n) > 0).map(n => n.path),
    [tree],
  );
  const expanded = useMemo(() => expandedDirs(rootDirs, selected, toggles), [rootDirs, selected, toggles]);

  useEffect(() => {
    if (!selected) return;
    document.querySelector('.sidebar-body .row.is-on')?.scrollIntoView({ block: 'nearest' });
  }, [selected, expanded]);

  const visibleFiles = useMemo(() => tree.flatMap(filePaths), [tree]);
  const allFiles = useMemo(
    () => collections.filter(c => !c.is_dir).map(c => c.path),
    [collections],
  );
  const startRun = useStore(s => s.startRun);
  const runJobId = useStore(s => s.runJobId);
  const checkAll = useStore(s => s.checkAll);
  const filtering = search.trim() !== '' || tagFilter.include.size > 0 || tagFilter.exclude.size > 0
    || runFilter !== 'all' || onlyChanged || onlyProblems;
  const setVisibleFiles = useStore(s => s.setVisibleFiles);
  useEffect(() => { setVisibleFiles(visibleFiles); }, [visibleFiles, setVisibleFiles]);

  const [showTags, setShowTags] = useState(false);
  const tagMenuRef = useDismiss<HTMLDivElement>(showTags, useCallback(() => setShowTags(false), []));
  const changedSince = useStore(s => s.changedSince);
  const changedAvailable = useStore(s => s.changedAvailable);
  const setChangedSince = useStore(s => s.setChangedSince);
  const chips = useMemo(
    () => activeFilters(tagFilter, onlyChanged, changedSince, onlyProblems),
    [tagFilter, onlyChanged, changedSince, onlyProblems],
  );

  const toggle = (path: string) => {
    setToggles(prev => toggleDir(prev, selected, path, expandedDirs(rootDirs, selected, prev).has(path)));
  };

  const cycleTag = (tag: string) => {
    setTagFilter(prev => {
      const include = new Set(prev.include);
      const exclude = new Set(prev.exclude);
      if (include.has(tag)) { include.delete(tag); exclude.add(tag); }
      else if (exclude.has(tag)) { exclude.delete(tag); }
      else { include.add(tag); }
      return { include, exclude };
    });
  };

  const handleContextMenu = useCallback((e: React.MouseEvent, node: TreeNode) => {
    e.preventDefault();
    setCtxMenu({ x: e.clientX, y: e.clientY, node });
  }, []);

  useEffect(() => {
    const handler = () => setCtxMenu(null);
    document.addEventListener('click', handler);
    return () => document.removeEventListener('click', handler);
  }, []);

  const askForName = async (
    title: string,
    question: string,
    kind: 'file' | 'folder',
    within: (typed: string) => string,
    attempt?: (path: string) => Promise<string | null>,
  ): Promise<string | null> => {
    let typed = '';
    let refusal: string | null = null;
    for (;;) {
      const asked = await modal.prompt(title, [refusal, question].filter(Boolean).join(' '), typed);
      if (asked === null || asked.trim() === '') return null;
      typed = asked;
      const path = within(asked.trim());
      refusal = createRefusal(path, collections.map(c => c.path), kind);
      if (refusal !== null) continue;
      if (!attempt) return path;
      refusal = await attempt(path);
      if (refusal === null) return path;
    }
  };

  const handleNewFolder = async (parentPath: string) => {
    const made = await askForName(
      'New Folder',
      'What it is called.',
      'folder',
      typed => (parentPath ? `${parentPath}/${typed}` : typed),
      async path => {
        try {
          const res = await fetch(`/api/dir/${apiPath(path)}`, { method: 'POST' });
          if (res.ok) return null;
          const said = await res.text().catch(() => '');
          return said.trim() || `${path} could not be created`;
        } catch {
          return 'The workbench could not be reached — nothing was created';
        }
      },
    );
    if (made) refreshCollections();
  };

  const handleNewFile = async (parentPath: string) => {
    const fullPath = await askForName(
      'New File',
      'File name — .gctf for gRPC, .httf for HTTP.',
      'file',
      typed => {
        const named = withFamilyExt(typed);
        return parentPath ? `${parentPath}/${named}` : named;
      },
      async path => {
        const fileName = path.split('/').pop()!;
        try {
          const res = await fetch('/api/save', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              path,
              content: newFileContent(familyOf(fileName) === 'httf' ? 'httf' : 'gctf'),
              create_only: true,
            }),
          });
          if (res.ok) return null;
          if (res.status === 409) return `${fileName} is already here.`;
          const said = await res.text().catch(() => '');
          return said.trim() || `The workbench could not write ${fileName}.`;
        } catch {
          return 'The workbench could not be reached — nothing was written.';
        }
      },
    );
    if (!fullPath) return;
    refreshCollections();
    void loadCollection(fullPath);
  };

  const openFile = useCallback((path: string, options?: { pin?: boolean }) => {
    void loadCollection(path, options).then(opened => {
      if (!opened) toast.error(openRefusal(path) ?? `${path} could not be opened — it may have been renamed or removed`);
    });
  }, [loadCollection, toast]);

  const handleMove = useCallback(async (fromPath: string, toPath?: string) => {
    if (toPath !== undefined) {
      const said = moveRefusal(fromPath, toPath, collections.map(c => c.path));
      if (said !== null) { toast.refuse(said); return; }
    }
    let to = toPath ?? null;
    let typed = fromPath;
    let refusal: string | null = null;
    while (to === null) {
      const asked = await modal.prompt(
        'Move / Rename',
        [refusal, 'Where it goes — the folders it lives in, and its name.'].filter(Boolean).join(' '),
        typed,
      );
      if (asked === null) return;
      typed = asked;
      refusal = moveRefusal(fromPath, asked, collections.map(c => c.path));
      if (refusal === null) to = asked;
    }
    if (!to?.trim()) return;
    if (to.trim() === fromPath) return;
    const named = await fetch(`/api/references/${apiPath(fromPath)}`)
      .then(r => (r.ok ? r.json() as Promise<string[]> : []))
      .catch(() => []);
    const note = [renameNote(fromPath, to.trim()), renameBreaksNote(named)]
      .filter(Boolean).join(' ');
    if (note) {
      const ok = await modal.confirm(`${fromPath} → ${to.trim()}`, note, { confirmText: 'rename' });
      if (!ok) return;
    }
    try {
      const res = await fetch('/api/move', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ from: fromPath, to: to.trim() }),
      });
      if (!res.ok) {
        const said = await res.text().catch(() => '');
        toast.error(said.trim() || `The workbench could not move ${fromPath}`);
        return;
      }
      const moved = await res.json().catch(() => null) as { rewritten?: string[] } | null;
      const rewritten = moved?.rewritten ?? [];
      if (rewritten.length > 0) {
        toast.info(rewritten.length === 1
          ? `${to.trim()} — ${rewritten[0]}`
          : `${to.trim()} — ${rewritten.length} paths respelled for its new folder`);
      }
      useStore.getState().retargetPath(fromPath, to.trim());
      refreshCollections();
    } catch { toast.error('The workbench could not be reached — nothing was moved'); }
  }, [collections, modal, refreshCollections, toast]);

  const [dragOverPath, setDragOverPath] = useState<string | null>(null);

  const handleDragStart = useCallback((e: React.DragEvent, node: TreeNode) => {
    if (node.isDir) { e.preventDefault(); return; }
    e.dataTransfer.setData('text/plain', node.path);
    e.dataTransfer.effectAllowed = 'move';
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent, node: TreeNode) => {
    if (!node.isDir) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = 'move';
    setDragOverPath(node.path);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    if (!e.currentTarget.contains(e.relatedTarget as Node)) {
      setDragOverPath(null);
    }
  }, []);

  const handleDrop = useCallback((e: React.DragEvent, node: TreeNode) => {
    e.preventDefault();
    setDragOverPath(null);
    if (!node.isDir) return;
    const fromPath = e.dataTransfer.getData('text/plain');
    if (!fromPath) return;
    const fileName = fromPath.split('/').pop() || fromPath;
    handleMove(fromPath, `${node.path}/${fileName}`);
  }, [handleMove]);

  return (
    <div className="stack rail-panel">
      <div className="field-frame">
        <Search size={12} className="muted inset-start no-shrink" />
        <input className="field" value={search} onChange={e => setSearch(e.target.value)} placeholder="Filter by name or tag…" />
        {search
          ? (
            <button className="btn is-ghost is-icon" onClick={() => setSearch('')} aria-label="Clear filter">
              <X size={11} />
            </button>
          )
          : (
            <button className="btn is-ghost is-icon" onClick={refreshCollections} title="Refresh" aria-label="Refresh">
              <RefreshCw size={12} />
            </button>
          )}
        {(allTags.length > 0 || changedAvailable || Object.keys(checked).length > 0) && (
          <div ref={tagMenuRef} className="picker">
            <button
              className={`btn is-ghost is-icon${chips.length > 0 ? ' is-on' : ''}`}
              onClick={() => setShowTags(v => !v)}
              title="Filter by tag"
              aria-label="Filter by tag"
              aria-haspopup="menu"
              aria-expanded={showTags}
            >
              <Tags size={12} />
            </button>
            <Popover open={showTags} anchor={tagMenuRef} align="end" className="tag-menu">
              <div className="menu">
                {allTags.map(({ tag, count }) => {
                  const wanted = tagFilter.include.has(tag);
                  const refused = tagFilter.exclude.has(tag);
                  return (
                    <button
                      key={tag}
                      className={`menu-item${wanted ? ' is-on' : ''}`}
                      onClick={() => cycleTag(tag)}
                      title={wanted ? 'Click to exclude' : refused ? 'Click to clear' : 'Click to require'}
                    >
                      <span className="grow">{wanted ? '+' : refused ? '−' : ''}{tag}</span>
                      <span className="muted">{count}</span>
                    </button>
                  );
                })}
                {Object.keys(checked).length > 0 && (
                  <>
                    <div className="menu-sep" />
                    <button
                      className={`menu-item${onlyProblems ? ' is-on' : ''}`}
                      onClick={() => setOnlyProblems(v => !v)}
                      title="Only the files the last check found something in"
                    >
                      <span className="grow">with problems</span>
                      <span className="muted">{Object.keys(checked).length}</span>
                    </button>
                  </>
                )}
                {changedAvailable && (
                  <>
                    <div className="menu-sep" />
                    <button
                      className={`menu-item${onlyChanged ? ' is-on' : ''}`}
                      onClick={() => setOnlyChanged(v => !v)}
                      disabled={(changedPaths?.length ?? 0) === 0}
                      title={(changedPaths?.length ?? 0) === 0
                        ? `Nothing differs from ${changedSince ?? 'HEAD'}`
                        : `The files that differ from ${changedSince ?? 'HEAD'} — the same selection \`run --only-changed\` makes`}
                    >
                      <span className="grow">only changed</span>
                      <span className="muted">{changedPaths?.length ?? 0}</span>
                    </button>
                    <label className="menu-item is-field">
                      <span className="muted">since</span>
                      <input
                        className="field mono"
                        defaultValue={changedSince ?? 'HEAD'}
                        spellCheck={false}
                        aria-label="Compare against"
                        title="A branch, a tag or a commit — the same ref `run --since` takes"
                        onKeyDown={e => {
                          if (e.key !== 'Enter') return;
                          e.preventDefault();
                          setChangedSince((e.currentTarget as HTMLInputElement).value);
                        }}
                        onBlur={e => setChangedSince(e.currentTarget.value)}
                      />
                    </label>
                  </>
                )}
              </div>
            </Popover>
          </div>
        )}
      </div>

      {chips.length > 0 && (
        <div className="wrap rail-chips">
          {chips.map(chip => (
            <button
              key={chip.key}
              className={`chip is-on${chip.kind === 'exclude' ? ' is-off' : ''}`}
              onClick={() => {
                if (chip.kind === 'changed') { setOnlyChanged(false); return; }
                if (chip.kind === 'problems') { setOnlyProblems(false); return; }
                const tag = chip.label.slice(1);
                setTagFilter(prev => {
                  const include = new Set(prev.include);
                  const exclude = new Set(prev.exclude);
                  include.delete(tag);
                  exclude.delete(tag);
                  return { include, exclude };
                });
              }}
              title="Click to drop this filter"
            >
              {chip.label}
              <X size={9} />
            </button>
          ))}
        </div>
      )}

      {filtering && (
        <div className="rail-filtered">
          <span className="grow">{visibleFiles.length} of {collections.length}</span>
          <button
            className="btn is-sm is-ghost"
            onClick={() => {
              setSearch('');
              setTagFilter({ include: new Set(), exclude: new Set() });
              setOnlyChanged(false);
              setOnlyProblems(false);
              useStore.getState().setRunFilter('all');
            }}
          >
            clear
          </button>
        </div>
      )}

      <RunSummary />

      {workspaceName !== '' && tree.length > 0 && (
        <div className="rail-group" title="The directory this workbench is serving">{workspaceName}/</div>
      )}

      {tree.length === 0 && read === 'failed' && (
        <div className="empty-state stack is-centred">
          <span>The workbench did not answer — this is not an empty project</span>
          <button className="btn is-sm" onClick={() => void refreshCollections()}>
            <RefreshCw size={12} /> try again
          </button>
        </div>
      )}

      {tree.length === 0 && read === 'ok' && (
        <div className="empty-state stack is-centred">
          <span>{filtering ? 'No matches' : 'No test files here yet'}</span>
          {!filtering && (
            <button className="btn is-sm" onClick={() => handleNewFile('')}>
              <FilePlus size={12} /> new file
            </button>
          )}
        </div>
      )}

      <div className="tree" role="tree" aria-label="Collections">
        <TreeNodes nodes={tree} depth={0} expanded={expanded} onToggle={toggle} selected={selected} onSelect={openFile} firstPath={tree[0]?.path ?? null}
          onContextMenu={handleContextMenu} onNewFolder={handleNewFolder} onNewFile={handleNewFile} onMove={handleMove}
          onDragStart={handleDragStart} onDragOver={handleDragOver} onDragLeave={handleDragLeave} onDrop={handleDrop}
          dragOverPath={dragOverPath} />
      </div>

      {ctxMenu && (
        <ContextMenu at={ctxMenu} onClose={() => setCtxMenu(null)}>
          {(() => {
            const shown = runTargets(ctxMenu.node, visibleFiles);
            const targets = runTargets(ctxMenu.node, visibleFiles, allFiles);
            const fixtures = targets.length - shown.length;
            const label = ctxMenu.node.isDir
              ? `Run ${count(shown.length, 'file')}${fixtures > 0 ? ` + ${count(fixtures, 'fixture')}` : ''}`
              : 'Run this file';
            return (
              <>
                <button
                  className="menu-item"
                  disabled={targets.length === 0 || runJobId !== null}
                  title={runJobId !== null
                    ? 'A run is already going'
                    : targets.length === 0
                      ? 'Nothing here to run — the rail is showing no test file inside it'
                      : `${targets.length === 1 ? targets[0] : `${ctxMenu.node.path}/`} — read from disk, the way CI reads it`}
                  onClick={() => { void startRun(targets); setCtxMenu(null); }}
                >
                  <Play size={13} /> {label}
                </button>
                <button
                  className="menu-item"
                  disabled={shown.length === 0}
                  title={shown.length === 0
                    ? 'Nothing here to check'
                    : `${count(shown.length, 'file')} — read and checked, without calling anything`}
                  onClick={() => {
                    setCtxMenu(null);
                    void checkAll(shown).then(() => {
                      const said = useStore.getState().checkedSaid;
                      if (said) toast.info(said);
                    });
                  }}
                >
                  <ShieldCheck size={13} /> {ctxMenu.node.isDir ? `Check ${count(shown.length, 'file')}` : 'Check this file'}
                </button>
                <div className="menu-sep" />
              </>
            );
          })()}
          {ctxMenu.node.isDir && (
            <>
              <button className="menu-item" onClick={() => { handleNewFolder(ctxMenu.node.path); setCtxMenu(null); }}>
                <FolderPlus size={13} /> New folder
              </button>
              <button className="menu-item" onClick={() => { handleNewFile(ctxMenu.node.path); setCtxMenu(null); }}>
                <FileJson size={13} /> New file
              </button>
              <div className="menu-sep" />
            </>
          )}
          <button className="menu-item" onClick={() => { handleMove(ctxMenu.node.path); setCtxMenu(null); }}>
            <Pencil size={13} /> Rename / Move
          </button>
          {!ctxMenu.node.isDir && (
            <>
              <button
                className="menu-item"
                onClick={() => {
                  const path = ctxMenu.node.path;
                  const held = useStore.getState().tabs.find(t => t.collectionPath === path);
                  const dirty = held ? isTabDirty(held) : false;
                  void useStore.getState().duplicateCollection(path)
                    .then(name => { if (name) toast.success(copiedNote(name, path, dirty)); })
                    .catch((e: Error) => toast.error(e.message));
                  setCtxMenu(null);
                }}
              >
                <CopyPlus size={13} /> Duplicate
              </button>
              <button
                className="menu-item"
                onClick={() => {
                  const path = ctxMenu.node.path;
                  void copyToClipboard(path)
                    .then(() => toast.success(`${path} copied`))
                    .catch(() => toast.error('The browser refused the clipboard'));
                  setCtxMenu(null);
                }}
              >
                <Copy size={13} /> Copy path
              </button>
            </>
          )}
          <div className="menu-sep" />
          <>
              <button
                className="menu-item is-danger"
                onClick={async () => {
                  const scope = deleteScope(
                    ctxMenu.node,
                    useStore.getState().tabs.map(t => ({ path: t.collectionPath, dirty: isTabDirty(t) })),
                  );
                  const note = unsavedNote(scope);
                  const named = await fetch(`/api/references/${apiPath(ctxMenu.node.path)}`)
                    .then(r => (r.ok ? r.json() as Promise<string[]> : []))
                    .catch(() => []);
                  const ok = await modal.confirm(
                    'Delete',
                    [deleteQuestion(ctxMenu.node, scope), note, referencedNote(named)]
                      .filter(Boolean).join(' '),
                    { confirmText: 'Delete', cancelText: 'Cancel', danger: true },
                  );
                  if (ok) {
                    const path = ctxMenu.node.path;
                    void fetch(`/api/collections/${apiPath(path)}`, { method: 'DELETE' })
                      .then(async r => {
                        if (r.ok) { refreshCollections(); return; }
                        const said = await r.text().catch(() => '');
                        toast.error(said.trim() || `${path} could not be deleted (${r.status})`);
                      })
                      .catch(() => toast.error('The workbench could not be reached'));
                  }
                  setCtxMenu(null);
                }}
              >
                <Trash2 size={13} /> Delete
              </button>
          </>
        </ContextMenu>
      )}
    </div>
  );
}

function RunFailure({ verdict, depth, onOpen }: {
  verdict: Verdict;
  depth: number;
  onOpen: (path: string) => void;
}) {
  const line = failureLine(verdict);
  const said = useStore(s => reasonsSaidOnce(s.run.verdicts));
  if (!line) return null;

  const failed = (verdict.assertions ?? []).filter(a => !a.passed);
  const more = Math.max(0, failed.length - 1);
  const undefined_names = unresolvedNames(verdict.message ?? line.text ?? '');

  const spells = railSaysWhy(verdict.message, said, undefined_names.length > 0);
  if (!spells) return null;

  return (
    <div className="run-failure-row" style={{ ['--depth' as string]: depth } as React.CSSProperties}>
    <button
      className="run-failure"
      onClick={() => onOpen(verdict.path)}
      title={[
        line.text,
        line.detail,
        verdict.message,
        verdict.durationMs !== undefined ? `took ${durationLabel(verdict.durationMs)}` : null,
        'Open the file — the response panel shows what came back',
      ].filter(Boolean).join('\n')}
    >
      <span className="assert-mark">✗</span>
      {verdict.caseLabel && <span className="badge is-info mono">{verdict.caseLabel}</span>}
      <span className="mono run-failure-line">
        {line.text}
        {line.detail && <span className="muted"> — {line.detail}</span>}
        {line.line !== null && <span className="muted"> line {line.line}</span>}
      </span>
      {more > 0 && <span className="muted">+{more}</span>}
      {moreRowsNote(verdict) && <span className="muted">{moreRowsNote(verdict)}</span>}
    </button>
    {undefined_names.length > 0 && (
      <button
        className="btn is-ghost is-sm run-failure-fix"
        title={`Give ${undefined_names.map(n => `{{${n}}}`).join(' and ')} a value in the environment that is switched on`}
        onClick={() => useStore.getState().openEnvManager(undefined_names[0])}
      >
        define
      </button>
    )}
    </div>
  );
}

function filePaths(node: TreeNode): string[] {
  if (!node.isDir) return [node.path];
  return node.children.flatMap(filePaths);
}

function TreeNodes({ nodes, depth, expanded, onToggle, selected, onSelect, onContextMenu, onNewFolder, onNewFile, onMove, onDragStart, onDragOver, onDragLeave, onDrop, dragOverPath, firstPath }: {
  nodes: TreeNode[]; depth: number; expanded: Set<string>; selected: string | null; firstPath: string | null;
  onToggle: (p: string) => void; onSelect: (p: string, options?: { pin?: boolean }) => void;
  onContextMenu: (e: React.MouseEvent, node: TreeNode) => void;
  onNewFolder: (parentPath: string) => void;
  onNewFile: (parentPath: string) => void;
  onMove: (fromPath: string) => void;
  onDragStart?: (e: React.DragEvent, node: TreeNode) => void;
  onDragOver?: (e: React.DragEvent, node: TreeNode) => void;
  onDragLeave?: (e: React.DragEvent) => void;
  onDrop?: (e: React.DragEvent, node: TreeNode) => void;
  dragOverPath?: string | null;
}) {
  return <>{nodes.map(node => (
    <TreeNodeRow key={node.path} node={node} depth={depth} expanded={expanded} firstPath={firstPath}
      onToggle={onToggle} selected={selected} onSelect={onSelect}
      onContextMenu={onContextMenu} onNewFolder={onNewFolder} onNewFile={onNewFile} onMove={onMove}
      onDragStart={onDragStart} onDragOver={onDragOver} onDragLeave={onDragLeave} onDrop={onDrop}
      dragOverPath={dragOverPath} />
  ))}</>;
}

function TreeNodeRow({ node, depth, expanded, onToggle, selected, onSelect, onContextMenu, onNewFolder, onNewFile, onMove, onDragStart, onDragOver, onDragLeave, onDrop, dragOverPath, firstPath }: {
  node: TreeNode; depth: number; expanded: Set<string>; selected: string | null; firstPath: string | null;
  onToggle: (p: string) => void; onSelect: (p: string, options?: { pin?: boolean }) => void;
  onContextMenu: (e: React.MouseEvent, node: TreeNode) => void;
  onNewFolder: (parentPath: string) => void;
  onNewFile: (parentPath: string) => void;
  onMove: (fromPath: string) => void;
  onDragStart?: (e: React.DragEvent, node: TreeNode) => void;
  onDragOver?: (e: React.DragEvent, node: TreeNode) => void;
  onDragLeave?: (e: React.DragEvent) => void;
  onDrop?: (e: React.DragEvent, node: TreeNode) => void;
  dragOverPath?: string | null;
}) {
  const isExpanded = expanded.has(node.path);
  const isSelected = !node.isDir && selected === node.path;
  const verdicts = useStore(s => s.run.verdicts);
  const checked = useStore(s => s.checked);
  const upToStep = useStore(s => s.run.upToStep);
  const verdict = node.isDir ? undefined : verdicts[node.path];
  const family = familyOf(node.name);
  const role = node.isDir ? null : fixtureRole(node.name);
  const iconClass = [verdict ? `is-${verdict.state}` : '', `is-${family}`].filter(Boolean).join(' ');
  const folderRun = useMemo(
    () => (node.isDir ? rollUp(filePaths(node), verdicts) : null),
    [node, verdicts],
  );
  const folderChecks = useMemo(
    () => (node.isDir ? rollUpChecks(filePaths(node), checked) : null),
    [node, checked],
  );
  const isDragOver = !node.isDir ? false : dragOverPath === node.path;
  const ranSomething = Object.keys(verdicts).length > 0;
  const untouched = ranSomething && !node.isDir && !verdict;
  const files = node.isDir ? fileCount(node) : 1;

  return (
    <>
      <div
        className={`row tree-node${isSelected ? ' is-on' : ''}${isDragOver ? ' is-drop' : ''}${untouched ? ' is-untouched' : ''}${node.isDir && files === 0 ? ' is-hollow' : ''}`}
        style={{ ['--depth' as string]: depth } as React.CSSProperties}
        role="treeitem"
        tabIndex={rowIsTabStop(node.path, selected, firstPath) ? 0 : -1}
        aria-level={depth + 1}
        aria-selected={node.isDir ? undefined : isSelected}
        aria-expanded={node.isDir ? isExpanded : undefined}
        onClick={() => { if (node.isDir) onToggle(node.path); else onSelect(node.path); }}
        onDoubleClick={() => { if (!node.isDir) onSelect(node.path, { pin: true }); }}
        onKeyDown={e => {
          if (e.key === 'Enter' || e.key === ' ') {
            e.preventDefault();
            if (node.isDir) onToggle(node.path); else onSelect(node.path);
            return;
          }
          if (e.key === 'ArrowRight' && node.isDir && !isExpanded) {
            e.preventDefault();
            onToggle(node.path);
            return;
          }
          if (e.key === 'ArrowLeft' && node.isDir && isExpanded) {
            e.preventDefault();
            onToggle(node.path);
            return;
          }
          const step = treeStep(e.key);
          if (step === null) return;
          e.preventDefault();
          moveRowFocus(e.currentTarget as HTMLElement, step, '.row.tree-node');
        }}
        onContextMenu={e => onContextMenu(e, node)}
        draggable={!node.isDir}
        onDragStart={e => onDragStart?.(e, node)}
        onDragOver={e => onDragOver?.(e, node)}
        onDragLeave={e => onDragLeave?.(e)}
        onDrop={e => onDrop?.(e, node)}
      >
        {node.isDir
          ? <ChevronRight size={10} className={isExpanded ? 'tree-caret is-open' : 'tree-caret'} />
          : <span className="tree-caret" />}
        {node.isDir
          ? (isExpanded ? <FolderOpen size={13} /> : <Folder size={13} />)
          : family === 'httf'
            ? <Globe size={13} className={iconClass} />
            : family === 'apif'
              ? <Network size={13} className={iconClass} />
              : <FileJson size={13} className={iconClass} />}
        <span
          className="row-name"
          title={
            node.isDir && files === 0 ? `${node.path}\nNo test files in here`
            : role ? `${node.path}\n${role === 'setup'
                ? 'Runs before the tests in this folder, and what it binds seeds them'
                : 'Runs after the tests in this folder, whatever they did'}`
            : node.tags?.length ? `${node.path}\n${node.tags.join(', ')}`
            : node.path
          }
        >
          {node.name}
        </span>
        {role && <span className="row-role">{role}</span>}
        <span className="row-tail">
          {node.isDir && (folderRun && folderRun.touched > 0
            ? <span className="row-note mono">
                {folderRun.passed > 0 && <span className="count is-ok">✓ {folderRun.passed}</span>}
                {folderRun.failed > 0 && <span className="count is-fail"> ✗ {folderRun.failed}</span>}
              </span>
            : files > 0 ? <span className="row-note">{files}</span> : null)}
          {node.isDir && folderChecks && folderChecks.files > 0 && (
            <span
              className={`row-note mono is-check is-${folderChecks.errors > 0 ? 'fail' : 'warn'}`}
              title={`${count(folderChecks.files, 'file')} inside `
                + `carry ${folderChecks.errors > 0 ? `${count(folderChecks.errors, 'error')}` : ''}`
                + `${folderChecks.errors > 0 && folderChecks.warnings > 0 ? ' and ' : ''}`
                + `${folderChecks.warnings > 0 ? `${count(folderChecks.warnings, 'warning')}` : ''}`}
            >
              <ShieldAlert size={9} />{folderChecks.files}
            </span>
          )}
          {verdict && (
            <span
              className={`row-note mono is-${verdict.state}`}
              title={[
                verdict.caseLabel,
                verdict.address && `went to ${verdict.address}`,
                verdict.message,
              ].filter(Boolean).join(' — ') || undefined}
            >
              {verdictLabel(verdict, upToStep)}
            </span>
          )}
          {!node.isDir && checkMark(checked[node.path]) && (
            <span
              className={`row-note mono is-check is-${checkMark(checked[node.path])!.kind === 'error' ? 'fail' : 'warn'}`}
              title={checkMark(checked[node.path])!.title}
            >
              <ShieldAlert size={9} />{checkMark(checked[node.path])!.label}
            </span>
          )}
          {verdict?.state === 'fail' && slowNote(verdict.durationMs) && (
            <span className="row-note mono is-slow" title={`This file took ${durationLabel(verdict.durationMs!)} to fail`}>
              {slowNote(verdict.durationMs)}
            </span>
          )}
          <span className="row-acts">
            {node.isDir && (
              <button
                className="btn is-ghost is-icon is-sm"
                onClick={e => { e.stopPropagation(); onNewFile(node.path); }}
                title={`New file in ${node.name}`}
                aria-label={`New file in ${node.name}`}
              >
                <FilePlus size={12} />
              </button>
            )}
            <button
              className="btn is-ghost is-icon is-sm"
              onClick={e => {
                e.stopPropagation();
                const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
                onContextMenu(
                  { preventDefault: () => {}, clientX: Math.max(8, rect.right - 150), clientY: rect.bottom + 4 } as React.MouseEvent,
                  node,
                );
              }}
              title="Rename, move, delete…"
              aria-label="More"
            >
              <MoreHorizontal size={12} />
            </button>
          </span>
        </span>
      </div>
      {verdict?.state === 'fail' && <RunFailure verdict={verdict} depth={depth} onOpen={onSelect} />}
      {node.isDir && isExpanded && <div role="group"><TreeNodes nodes={node.children} depth={depth + 1} expanded={expanded} firstPath={firstPath} onToggle={onToggle} selected={selected} onSelect={onSelect} onContextMenu={onContextMenu} onNewFolder={onNewFolder} onNewFile={onNewFile} onMove={onMove} onDragStart={onDragStart} onDragOver={onDragOver} onDragLeave={onDragLeave} onDrop={onDrop} dragOverPath={dragOverPath} /></div>}
    </>
  );
}
