import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { isTabDirty, useStore } from '../../lib/store';
import { useToast } from 'luvo/ui/ToastContext';
import { benchLine, caseNote, coverageNote, dataFiles, scopeFiles, type DataFile, unsavedAmong, untestedNames } from '../../lib/jobs';
import { downloadableReports } from '../../lib/reports';
import { failureGroups, reasonsSaidOnce } from '../../lib/tree';
import { reconcileChoice } from '../../lib/run-data';
import { durationLabel } from '../../lib/format';
import { useDismiss } from 'luvo/input/useDismiss';
import { Popover } from 'luvo/ui/Popover';
import type { RunScope } from '../../lib/types';
import { BookText, Play, Square, ChevronDown, Check, Download, FilePlus2, FolderOpen, Gauge, Loader2, RotateCcw, Table2 } from 'lucide-react';
import { DocsDialog } from './DocsDialog';
import { REPORT_FORMATS, isDirectoryReport, reportRefusal, reportsDirOf, writeNote } from '../../lib/reports';
import { benchRefusal, benchTakes } from '../../lib/tree';
import { count } from 'luvo/data/plural';
import { copyToClipboard } from 'luvo/data/clipboard';

const FORMATS = REPORT_FORMATS;

const SCOPES: { key: RunScope; label: string; hint: string }[] = [
  { key: 'file', label: 'this file', hint: 'The file open in the active tab' },
  { key: 'folder', label: 'this folder', hint: 'Every file beside it' },
  { key: 'all', label: 'everything', hint: 'Every file the list is showing' },
];

export function RunControl() {
  const files = useStore(s => s.visibleFiles);
  const workspacePath = useStore(s => s.workspacePath);
  const scope = useStore(s => s.runScope);
  const setScope = useStore(s => s.setRunScope);
  const run = useStore(s => s.run);
  const runJobId = useStore(s => s.runJobId);
  const startRun = useStore(s => s.startRun);
  const cancelRun = useStore(s => s.cancelRun);

  const formats = useStore(s => s.reportFormats);
  const toggleFormat = useStore(s => s.toggleReportFormat);
  const reportsDir = useStore(s => reportsDirOf(s.projectRoot));
  const startBench = useStore(s => s.startBench);
  const hasBench = useStore(s => Object.keys(s.collectionParsed?.bench ?? {}).length > 0);
  const benchNo = useStore(s => (s.workspacePath ? benchRefusal(s.workspacePath) : null));
  const benchable = useMemo(() => files.filter(benchTakes), [files]);
  const [menu, setMenu] = useState(false);
  const showDocs = useStore(s => s.docsOpen);
  const setShowDocs = useStore(s => s.setDocsOpen);
  const runData = useStore(s => s.runData);
  const setRunData = useStore(s => s.setRunData);
  const [sources, setSources] = useState<DataFile[]>([]);
  const toast = useToast();
  useEffect(() => { if (menu) void dataFiles().then(setSources); }, [menu]);
  const checked = useRef(false);
  useEffect(() => {
    if (checked.current || runData === null) return;
    checked.current = true;
    void dataFiles().then(list => {
      setSources(list);
      const held = reconcileChoice(runData, list);
      if (!held) {
        setRunData(null);
        toast.warn(`${runData} is not on disk any more — runs are no longer driven over it`);
        return;
      }
      setRunData(held.path, held.columns);
    }).catch(() => { checked.current = false; });
  }, [runData, setRunData, toast]);
  const menuRef = useDismiss<HTMLDivElement>(menu, useCallback(() => setMenu(false), []));
  const triggerRef = useRef<HTMLButtonElement>(null);
  const openMenu = () => setMenu(v => !v);

  const targets = useMemo(() => scopeFiles(files, scope, workspacePath), [files, scope, workspacePath]);
  const openTabs = useStore(s => s.tabs);
  const unsaved = useMemo(
    () => unsavedAmong(targets, openTabs.map(t => ({ path: t.collectionPath, dirty: isTabDirty(t) }))),
    [targets, openTabs],
  );
  const scopeLabel = SCOPES.find(s => s.key === scope)?.label ?? scope;

  const running = runJobId !== null && !run.finished;

  return (
    <div ref={menuRef} className="picker">
      {running ? (
        <button className="btn is-sm is-ghost is-icon is-danger" onClick={cancelRun} title="Cancel the run">
          <Square size={12} />
        </button>
      ) : (
        <div className="bar run-control">
          <button
            className="btn is-sm is-ghost"
            onClick={() => startRun(targets)}
            disabled={targets.length === 0}
            title={targets.length === 0
              ? 'Open a saved file, or widen the scope'
              : `Run ${scopeLabel} — ${count(targets.length, 'file')}${runData ? ` over the rows of ${runData}` : ''} (⌘⇧R)${unsaved.length > 0 ? `\n${unsaved.join(', ')} — read from disk, without the edits open here` : ''}`}
          >
            <Play size={12} /> {targets.length}
            {runData && (
              <span className="badge is-kind mono run-over" title={`Every file runs once per row of ${runData}`}>
                <Table2 size={10} /> {runData.split('/').pop()}
              </span>
            )}
            {unsaved.length > 0 && <span className="badge is-pending">{unsaved.length} unsaved</span>}
          </button>
          <button
            ref={triggerRef}
            className="btn is-sm is-ghost is-icon"
            onClick={openMenu}
            title={`Scope: ${scopeLabel}`}
            aria-haspopup="menu"
            aria-expanded={menu}
          >
            <ChevronDown size={11} />
          </button>
        </div>
      )}

      {showDocs && <DocsDialog paths={files} onClose={() => setShowDocs(false)} />}

      <Popover open={menu} anchor={menuRef} className="run-menu">
        <div className="menu">
          <div className="menu-group">run</div>
          {unsaved.length > 0 && (
            <div className="note is-warn">
              {unsaved.length === 1 ? `${unsaved[0]} has` : `${unsaved.length} of these files have`} edits
              that are not on disk. A run reads what is saved.
            </div>
          )}
          {SCOPES.map(s => {
            const scoped = scopeFiles(files, s.key, workspacePath).length;
            return (
              <button
                key={s.key}
                className={`menu-item${scope === s.key ? ' is-on' : ''}`}
                disabled={scoped === 0}
                onClick={() => {
                  setScope(s.key);
                  setMenu(false);
                  void startRun(scopeFiles(files, s.key, workspacePath));
                }}
                title={scoped === 0
                  ? `Nothing to run — ${s.hint.toLowerCase()}`
                  : `${s.hint} — runs ${count(scoped, 'file')} now`}
              >
                <Play size={12} />
                <span className="grow">{s.label}</span>
                <span className="muted">{scoped}</span>
              </button>
            );
          })}

          <button
            className="menu-item"
            onClick={() => {
              setMenu(false);
              void useStore.getState().checkAll(targets).then(() => {
                const said = useStore.getState().checkedSaid;
                if (said) toast.info(said);
              });
            }}
            disabled={targets.length === 0}
            title={`Check ${count(targets.length, 'file')} without calling anything`}
          >
            <span className="grow">check {scopeLabel}</span>
            <span className="muted">{targets.length}</span>
          </button>
          <div className="menu-sep" />
          {(sources.length > 0 || runData) && (
            <>
              <div className="menu-sep" />
              <div className="menu-group">over rows of</div>
              <button
                className={`menu-item${runData === null ? ' is-on' : ''}`}
                onClick={() => setRunData(null)}
                title="Run each file once, as it is written"
              >
                <span className="grow">no data source</span>
              </button>
              {runData !== null && !sources.some(s => s.path === runData) && (
                <button
                  className="menu-item is-on"
                  onClick={() => setRunData(null)}
                  title={`${runData} is not on disk any more — a run over it is refused`}
                >
                  <Table2 size={12} />
                  <span className="grow mono">{runData}</span>
                  <span className="badge is-pending">not on disk</span>
                </button>
              )}
              {sources.map(s => (
                <button
                  key={s.path}
                  className={`menu-item${runData === s.path ? ' is-on' : ''}`}
                  onClick={() => setRunData(s.path, s.columns ?? [])}
                  title={[
                    `${s.path} — every file runs once per row`,
                    s.columns?.length
                      ? `it answers ${s.columns.map(c => `{{${c}}}`).join(', ')}`
                      : `its columns are available as {{${s.name.replace(/\.[^.]+$/, '')}.column}}`,
                  ].join('\n')}
                >
                  <Table2 size={12} />
                  <span className="grow mono">{s.path}</span>
                  <span className="muted">{s.format}</span>
                </button>
              ))}
            </>
          )}

          <div className="menu-sep" />
          <div className="menu-group">measure</div>
          <button
            className="menu-item"
            disabled={!workspacePath || benchNo !== null || !hasBench}
            title={
              !workspacePath ? 'Open a saved file first'
              : benchNo !== null ? benchNo
              : !hasBench ? 'This file has no BENCH section — add one in config'
              : 'Run the file’s BENCH configuration'
            }
            onClick={() => { setMenu(false); void startBench(workspacePath!); }}
          >
            <Gauge size={12} /> <span className="grow">bench this file</span>
          </button>
          <button
            className="menu-item"
            disabled={benchable.length === 0}
            title={benchable.length === 0
              ? (files.length === 0 ? 'The rail is showing no files' : 'The load runner measures gRPC calls — the rail is showing no .gctf file')
              : 'One measurement over every .gctf the rail is showing — they must share a BENCH config'}
            onClick={() => { setMenu(false); void startBench(benchable); }}
          >
            <Gauge size={12} /> <span className="grow">bench what the rail shows</span>
            <span className="muted">{benchable.length}</span>
          </button>

          <div className="menu-sep" />
          <div className="menu-group">document</div>
          <button
            className="menu-item"
            disabled={files.length === 0}
            title={files.length === 0
              ? 'The rail is showing no files'
              : 'What `grpctestify docs` would write for these files — read before anything is written'}
            onClick={() => { setMenu(false); setShowDocs(true); }}
          >
            <BookText size={12} /> <span className="grow">API docs for what the rail shows</span>
            <span className="muted">{files.length}</span>
          </button>

          <div className="menu-sep" />
          <div className="menu-group">reports the next run writes</div>
          {FORMATS.map(f => (
            <button
              key={f}
              role="menuitemcheckbox"
              aria-checked={formats.includes(f)}
              className={`menu-item${formats.includes(f) ? ' is-on' : ''}`}
              onClick={() => toggleFormat(f)}
              title={writeNote(f, formats.includes(f), reportsDir)}
            >
              <span className="grow">{f}</span>
              {formats.includes(f) && <Check size={12} />}
            </button>
          ))}
        </div>
      </Popover>
    </div>
  );
}

async function saveReport(jobId: string, file: string): Promise<{ error?: string; note?: string }> {
  let res: Response;
  try {
    res = await fetch(`/api/jobs/${jobId}/report/${file}`);
  } catch {
    return { error: `${file} could not be fetched — the workbench could not be reached` };
  }
  if (!res.ok) return { error: reportRefusal(file, res.status) };
  if (isDirectoryReport(file)) {
    const said = await res.json().catch(() => null);
    if (!said?.path) return { error: `${file} was written, but the workbench could not read where` };
    const copied = await copyToClipboard(said.open).then(() => true).catch(() => false);
    return {
      note: `${count(said.files ?? 0, 'result')} in ${said.path}${copied ? ` — \`${said.open}\` copied` : ''}`,
    };
  }
  const url = URL.createObjectURL(await res.blob());
  const link = document.createElement('a');
  link.href = url;
  link.download = file;
  link.click();
  URL.revokeObjectURL(url);
  return {};
}

export function RunSummary() {
  const run = useStore(s => s.run);
  const runData = useStore(s => s.runData);
  const runJobId = useStore(s => s.runJobId);
  const filter = useStore(s => s.runFilter);
  const reports = useStore(s => s.lastReports);
  const runError = useStore(s => s.runError);
  const setFilter = useStore(s => s.setRunFilter);
  const running = runJobId !== null && !run.finished;
  const startRun = useStore(s => s.startRun);
  const toast = useToast();
  const save = async (jobId: string, file: string) => {
    const said = await saveReport(jobId, file);
    if (said.error) toast.error(said.error);
    else if (said.note) toast.success(said.note);
  };
  const failedPaths = useMemo(
    () => Object.values(run.verdicts).filter(v => v.state === 'fail').map(v => v.path),
    [run.verdicts],
  );

  if (runError) {
    const sourceGone = runData !== null && runError.includes('Data source not found');
    return (
      <div className="summary is-fail" title={runError}>
        <span className="grow">{runError}</span>
        {sourceGone && (
          <button
            className="btn is-sm is-ghost"
            onClick={() => { useStore.getState().setRunData(null); }}
            title={`Stop running over ${runData} — every file runs once, as it is written`}
          >
            run without it
          </button>
        )}
      </div>
    );
  }
  if (!running && run.done === 0 && !run.finished) return null;

  const pct = run.total > 0 ? Math.round((run.done / run.total) * 100) : 0;
  const benching = running ? benchLine(run) : null;
  const chip = (mode: 'pass' | 'fail' | 'skip', mark: string, n: number, tone: string) => (
    <button
      className={`count is-${tone}${filter === mode ? ' is-on' : ''}`}
      onClick={() => setFilter(filter === mode ? 'all' : mode)}
      disabled={n === 0}
      title={filter === mode ? 'Show every file again' : `Show only the ${mode === 'pass' ? 'passing' : mode === 'fail' ? 'failing' : 'skipped'} files`}
    >
      {mark} {n}
    </button>
  );

  return (
    <>
    <div className="summary run-summary" style={{ '--progress': `${pct}%` } as React.CSSProperties}>
      {running && run.lost === 0 && <span className="spinner" />}
      {run.lost > 0 && <span className="run-lost" title="The run is still going on the server">reconnecting</span>}
      {run.outcome === 'cancelled' && <span className="run-lost" title="Stopped before every file ran">cancelled</span>}
      {runData && <span className="muted mono run-data" title={`Driven over the rows of ${runData}`}>{runData.split('/').pop()}</span>}
      {caseNote(run) && (
        <span className="muted mono run-cases" title={caseNote(run)!.title}>{caseNote(run)!.label}</span>
      )}
      {benching
        ? <span className="mono run-bench" title={benching.title}>{benching.label}</span>
        : (
          <>
            {chip('pass', '✓', run.passed, 'ok')}
            {chip('fail', '✗', run.failed, 'fail')}
            {run.skipped > 0 && chip('skip', '∅', run.skipped, 'skip')}
          </>
        )}
      <span className="grow" />
      {run.finished && failedPaths.length > 0 && !running && (
        <button
          className="btn is-sm is-ghost is-icon"
          onClick={() => startRun(failedPaths)}
          title={`Run only the ${count(failedPaths.length, 'file')} that failed`}
          aria-label={`Run the ${failedPaths.length} that failed`}
        >
          <RotateCcw size={11} />
        </button>
      )}
      {run.upToStep !== undefined && (
        <span className="badge is-info" title="Run to here — the steps after this one were not run">
          {run.upToStep === 1 ? 'step 1' : `steps 1–${run.upToStep}`}
        </span>
      )}
      <CoverageChip />
      <span
        className="muted mono run-tail"
        title={run.workers && run.workers > 1
          ? `${count(run.total, 'test')} at ${run.workers} at a time — the width \`run\` uses`
          : undefined}
      >
        {running
          ? (benching ? '' : `${run.done}/${run.total}`)
          : run.durationMs > 0 ? durationLabel(run.durationMs) : ''}
      </span>
      {reports.jobId !== '' && !running && downloadableReports(reports.files).map(({ file, ready }) => {
        const dir = isDirectoryReport(file);
        return (
        <button
          key={file}
          className={`btn is-sm is-ghost run-report${ready ? '' : ' is-quiet'}`}
          onClick={() => void save(reports.jobId, file)}
          title={dir
            ? `${ready ? 'Written' : 'Write'} a directory of Allure results beside this run's reports — \`allure serve\` opens it, and the path is copied`
            : ready
            ? `Download ${file} — this run wrote it`
            : `Download ${file} — written from this run when you ask for it`}
          aria-label={dir ? `Where the Allure results are` : `Download ${file}`}
        >
          {dir ? <FolderOpen size={11} /> : <Download size={11} />} <span className="mono">{file}</span>
        </button>
        );
      })}
    </div>
    <FailureReasons />
    </>
  );
}

function FailureReasons() {
  const verdicts = useStore(s => s.run.verdicts);
  const finished = useStore(s => s.run.finished);
  const reason = useStore(s => s.runReason);
  const setReason = useStore(s => s.setRunReason);
  const groups = useMemo(() => failureGroups(verdicts), [verdicts]);

  if (!finished) return null;
  const said = reasonsSaidOnce(verdicts);
  if (said.size === 0) return null;

  const shown = groups.filter(g => said.has(g.reason));
  const rest = groups.length - shown.length;

  return (
    <div className="summary run-why">
      <span className="label">why</span>
      {shown.map(group => (
        <button
          key={group.reason}
          className={`count run-why-one${reason === group.reason ? ' is-on' : ''}`}
          onClick={() => setReason(reason === group.reason ? null : group.reason)}
          title={reason === group.reason
            ? `${group.reason}\n\nShow every file again`
            : `${group.reason}\n\nShow only the ${count(group.paths.length, 'file')} that failed this way`}
        >
          <span className="mono">{group.paths.length}</span> <span className="run-why-text">{group.reason}</span>
        </button>
      ))}
      {rest > 0 && <span className="muted">and {rest} more</span>}
    </div>
  );
}

function CoverageChip() {
  const coverage = useStore(s => s.run.coverage);
  const scaffoldTest = useStore(s => s.scaffoldTest);
  const toast = useToast();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const ref = useDismiss<HTMLDivElement>(open, useCallback(() => setOpen(false), []));
  const note = coverageNote(coverage);
  const untested = untestedNames(coverage);
  if (!note) return null;

  return (
    <div ref={ref} className="picker run-coverage-picker">
      <button
        className="badge run-coverage is-pick"
        onClick={() => setOpen(v => !v)}
        disabled={untested.length === 0}
        aria-haspopup={untested.length === 0 ? undefined : 'menu'}
        aria-expanded={untested.length === 0 ? undefined : open}
        title={note.title}
      >
        {note.label}
      </button>
      {open && untested.length > 0 && (
        <div className="menu coverage-menu" role="menu">
          <div className="menu-group">never called by this run — scaffold writes the file</div>
          {untested.map(name => (
            <button
              key={name}
              className="menu-item"
              disabled={busy !== null}
              onClick={async () => {
                setBusy(name);
                try {
                  await scaffoldTest(name);
                  setOpen(false);
                } catch (err: any) {
                  toast.error(err?.message || 'Nothing was scaffolded');
                } finally {
                  setBusy(null);
                }
              }}
            >
              <span className="mono grow">{name}</span>
              {busy === name ? <Loader2 size={11} className="animate-spin" /> : <FilePlus2 size={11} />}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
