import { useState, useMemo, useCallback, useEffect } from 'react';
import { answeredHere, openRefusal, useStore } from '../../lib/store';
import type { HistoryEntry } from '../../lib/types';
import { sortByRecency } from '../../lib/history-list';
import { burstKey, burstRepeats, callSummary, groupByDay, methodOf, msUntilMidnight, tookRange } from '../../lib/history-group';
import { useModal } from 'luvo/ui/useModal';
import { Trash2, Search, Play, X, MoreHorizontal, RefreshCw } from 'lucide-react';
import { useDismiss } from 'luvo/input/useDismiss';
import { readProjectHistory, type ProjectEntry } from '../../lib/project-history';
import { unansweredNow } from '../../lib/env';
import { SHAPE_LABEL, SHAPE_TONE, shapeOfName } from '../../lib/shape';
import { copyToClipboard } from 'luvo/data/clipboard';
import { runsTheWholeFile, commandLine } from '../../lib/docs';
import { useToast } from 'luvo/ui/useToast';
import { callAddress } from '../../lib/store';
import { HistoryPeek } from './HistoryPeek';
import { entryFailed } from '../../lib/call-outcome';
import { toCurl } from '../../lib/curl-import';
import { httpUrl, looksHttp, splitEndpoint, shortTarget } from '../../lib/http-endpoint';
import { httpStatusLabel, httpStatusTone, durationLabel } from '../../lib/format';
import { moveRowFocus, treeStep } from '../../lib/tree-keys';
import { count } from 'luvo/data/plural';

const PROJECT_HISTORY_CAP = 2000;

const pad = (n: number) => n.toString().padStart(2, '0');

function clock(ts: number) {
  const d = new Date(ts);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function stamp(ts: number) {
  const d = new Date(ts);
  return `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

type RowMenu = { x: number; y: number; entry: HistoryEntry };

function useToday(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = setTimeout(() => setNow(Date.now()), msUntilMidnight(now));
    return () => clearTimeout(id);
  }, [now]);
  return now;
}

export function HistoryPanel() {
  const history = useStore(s => s.history);
  const restoreHistory = useStore(s => s.restoreHistory);
  const loadCollection = useStore(s => s.loadCollection);
  const clearHistory = useStore(s => s.clearHistory);
  const forgetHistory = useStore(s => s.forgetHistory);
  const requestSaveAs = useStore(s => s.requestSaveAs);
  const execute = useStore(s => s.execute);
  const modal = useModal();
  const toast = useToast();
  const [search, setSearch] = useState('');
  const [failedOnly, setFailedOnly] = useState(false);
  const projectRoot = useStore(s => s.projectRoot);
  const [source, setSource] = useState<'browser' | 'project'>('browser');
  const [read, setRead] = useState<{ asked: number; entries: ProjectEntry[]; error: string | null } | null>(null);
  const [projectAsked, setProjectAsked] = useState(0);

  useEffect(() => {
    if (!projectRoot) return;
    let live = true;
    const asked = projectAsked;
    void readProjectHistory().then(said => {
      if (live) setRead({ asked, entries: said.entries, error: said.error });
    });
    return () => { live = false; };
  }, [projectRoot, projectAsked]);

  const answered = read !== null && read.asked === projectAsked;
  const projectError = answered ? read.error : null;

  const [opened, setOpened] = useState<Set<string>>(new Set());
  const toggle = useCallback((id: string) => setOpened(prev => {
    const next = new Set(prev);
    if (!next.delete(id)) next.add(id);
    return next;
  }), []);
  const [peek, setPeek] = useState<{ entry: HistoryEntry; top: number } | null>(null);
  const [menu, setMenu] = useState<RowMenu | null>(null);
  const showPeek = useCallback((entry: HistoryEntry, at: HTMLElement) => {
    const rect = at.getBoundingClientRect();
    const top = Math.max(8, Math.min(rect.top, window.innerHeight - 420));
    setPeek(prev => (prev?.entry.id === entry.id ? null : { entry, top }));
  }, []);
  const menuRef = useDismiss<HTMLDivElement>(menu !== null, useCallback(() => setMenu(null), []));
  const peekRef = useDismiss<HTMLDivElement>(peek !== null, useCallback(() => setPeek(null), []));

  const loadingProject = !!projectRoot && !answered;
  const projectRows = answered ? read.entries : [];
  const rows = source === 'project' ? projectRows : history;

  const matched = useMemo(() => {
    const needle = search.toLowerCase();
    return sortByRecency(
      rows.filter(h =>
        (!search || h.endpoint.toLowerCase().includes(needle) || h.bodies.join(' ').toLowerCase().includes(needle)) &&
        (!failedOnly || entryFailed(h))),
    );
  }, [rows, search, failedOnly]);

  const total = matched.length;
  const now = useToday();
  const failures = useMemo(() => rows.filter(entryFailed).length, [rows]);

  const replay = useCallback((entry: HistoryEntry) => {
    const path = entry.kind === 'run' ? entry.collectionPath : undefined;
    if (path) {
      void loadCollection(path, { pin: true }).then(opened => {
        if (opened) void useStore.getState().runTest();
        else toast.error(openRefusal(path) ?? `${path} is not in this workbench — it may have been renamed or removed`);
      });
      return;
    }
    restoreHistory(entry, { pin: true });
    const missing = unansweredNow(entry.resolved, answeredHere(useStore.getState()));
    if (missing.length > 0) {
      toast.warn(
        `Sending with ${missing.join(', ')} as written — ${missing.length === 1 ? 'that name was' : 'those names were'} resolved when this call was made, and nothing answers for ${missing.length === 1 ? 'it' : 'them'} here`,
      );
    }
    void execute();
  }, [restoreHistory, execute, loadCollection, toast]);

  const copyCommand = useCallback(async (kind: 'call' | 'grpcurl', entry: HistoryEntry) => {
    try {
      const command = await commandLine(kind, {
        endpoint: entry.endpoint,
        body: entry.bodies.length === 1 ? JSON.parse(entry.bodies[0] || 'null') : entry.bodies.map(b => JSON.parse(b || 'null')),
        address: entry.connection?.address,
        protocol: entry.connection?.protocol,
        tls: entry.connection?.tls,
        tls_insecure: entry.connection?.tlsInsecure,
        headers: Object.keys(entry.headers ?? {}).length > 0 ? entry.headers : undefined,
        collection_path: entry.kind === 'run' ? undefined : entry.collectionPath,
      });
      await copyToClipboard(command);
      const named = command.includes('{{');
      const runsTheFile = kind === 'call' && runsTheWholeFile(command);
      toast.success([
        kind === 'call' ? 'grpctestify call copied' : 'grpcurl copied',
        runsTheFile ? ' — it runs the file, so it sends what the file says now' : '',
        named ? ' — it still names variables the history did not keep values for' : '',
      ].join(''));
    } catch (err: any) {
      toast.error(err?.message || 'Could not build the command');
    }
  }, [toast]);

  const copyCurl = useCallback(async (entry: HistoryEntry) => {
    const { method, path } = splitEndpoint(entry.endpoint);
    const url = httpUrl(entry.connection?.address ?? '', path);
    try {
      const line = toCurl({
        method,
        url,
        headers: entry.headers ?? {},
        body: entry.bodies.find(b => b.trim()) ?? '',
      });
      await copyToClipboard(line);
      toast.success(line.includes('{{')
        ? 'curl copied — it still names variables the history did not keep values for'
        : 'curl copied');
    } catch {
      toast.error('The browser refused the clipboard');
    }
  }, [toast]);

  const keepAsFile = useCallback((entry: HistoryEntry) => {
    restoreHistory(entry, { pin: true });
    requestSaveAs();
  }, [restoreHistory, requestSaveAs]);

  return (
    <div className="stack history">
      {(matched.length > 0 || search) && (
        <div className="field-frame">
          <Search size={12} className="muted history-search-mark" />
          <input
            className="field"
            value={search}
            onChange={e => setSearch(e.target.value)}
            placeholder="Filter by endpoint…"
          />
          {search && (
            <button className="btn is-ghost is-icon" onClick={() => setSearch('')} aria-label="Clear filter">
              <X size={11} />
            </button>
          )}
        </div>
      )}

      {(projectRoot || history.length > 0) && (
        <div className="bar history-filters">
          {projectRoot && (
            <>
              <button
                className={`chip${source === 'browser' ? ' is-on' : ''}`}
                onClick={() => setSource('browser')}
                title="What this browser has called, kept in its own storage"
              >
                browser <span className="chip-count">{history.length}</span>
              </button>
              <button
                className={`chip${source === 'project' ? ' is-on' : ''}`}
                onClick={() => setSource('project')}
                title={`Every session the project recorded — .grpctestify/history, credentials redacted${projectRows.length >= PROJECT_HISTORY_CAP ? `. The newest ${PROJECT_HISTORY_CAP} are shown.` : ''}`}
              >
                project {loadingProject
                  ? <span className="muted">…</span>
                  : <span className="chip-count">{projectRows.length >= PROJECT_HISTORY_CAP ? `${PROJECT_HISTORY_CAP}+` : projectRows.length}</span>}
              </button>
            </>
          )}
          {failures > 0 && (
            <button
              className={`chip is-fail-chip${failedOnly ? ' is-on' : ''}`}
              onClick={() => setFailedOnly(v => !v)}
              title={failedOnly ? 'Show every call again' : 'Only the calls that came back with an error'}
            >
              failed <span className="chip-count">{failures}</span>
            </button>
          )}
          <span className="grow" />
          {source === 'project' && (
            <button
              className="btn is-ghost is-icon is-sm history-reread"
              onClick={() => setProjectAsked(n => n + 1)}
              disabled={loadingProject}
              title="Read the project’s record again — it grows as runs and other sessions call"
              aria-label="Read the project’s record again"
            >
              <RefreshCw size={12} />
            </button>
          )}
          {source === 'browser' && (
            <button
              className="btn is-ghost is-icon is-sm"
              onClick={async () => {
                const ok = await modal.confirm(
                  `Clear ${count(history.length, 'call')}?`,
                  'This history is only in this browser, and clearing it cannot be undone.',
                  { confirmText: 'clear', cancelText: 'keep', danger: true },
                );
                if (ok) clearHistory();
              }}
              title="Clear this browser’s history"
              aria-label="Clear this browser’s history"
            >
              <Trash2 size={12} />
            </button>
          )}
        </div>
      )}

      {total === 0 && (
        <div className="empty-state">
          {search ? 'No matches'
            : failedOnly ? 'Nothing failed'
            : source === 'project'
              ? loadingProject ? 'Reading the project’s record…'
              : projectError
                ? (
                  <>
                    <span className="history-project-error">{projectError}</span>
                    <button className="btn is-sm is-ghost" onClick={() => setProjectAsked(n => n + 1)}>try again</button>
                  </>
                )
                : 'The project has recorded no calls yet'
            : 'No calls yet'}
        </div>
      )}

      {menu && (
        <div ref={menuRef} className="menu history-menu" style={{ left: menu.x, top: menu.y }}>
          <button className="menu-item" onClick={() => { restoreHistory(menu.entry, { pin: true }); setMenu(null); }}>
            {menu.entry.kind === 'run' ? 'Open the file' : 'Open as a tab'}
          </button>
          {menu.entry.kind !== 'run' && menu.entry.collectionPath && (
            <button
              className="menu-item"
              onClick={() => {
                const path = menu.entry.collectionPath!;
                void loadCollection(path, { pin: true }).then(opened => {
                  if (!opened) toast.error(openRefusal(path) ?? `${path} is not in this workbench — it may have been renamed or removed`);
                });
                setMenu(null);
              }}
              title={`Open ${menu.entry.collectionPath}`}
            >
              Open the file it was made from
            </button>
          )}
          {menu.entry.kind !== 'run' && (
            <button
              className="menu-item"
              onClick={() => { keepAsFile(menu.entry); setMenu(null); }}
              title="Open it as a tab and choose where to write it"
            >
              Keep as a file…
            </button>
          )}
          <button className="menu-item" onClick={() => { replay(menu.entry); setMenu(null); }}>
            {menu.entry.kind === 'run' ? 'Run it again' : 'Send it again'}
          </button>
          <div className="menu-sep" />
          {menu.entry.kind === 'run' ? (
            <button
              className="menu-item"
              onClick={() => {
                const path = menu.entry.collectionPath!;
                void copyToClipboard(`grpctestify run ${path}`)
                  .then(() => toast.success('grpctestify run copied'))
                  .catch(() => toast.error('The browser refused the clipboard'));
                setMenu(null);
              }}
              title="The command that runs this file"
            >
              Copy as grpctestify run
            </button>
          ) : (
            looksHttp(menu.entry.endpoint) ? (
              <button
                className="menu-item"
                onClick={() => { void copyCurl(menu.entry); setMenu(null); }}
                title="The same call as a curl command line"
              >
                Copy as curl
              </button>
            ) : (
              <>
                <button
                  className="menu-item"
                  onClick={() => { void copyCommand('call', menu.entry); setMenu(null); }}
                  title="grpctestify call, with this call's target and body"
                >
                  Copy as grpctestify call
                </button>
                <button
                  className="menu-item"
                  onClick={() => { void copyCommand('grpcurl', menu.entry); setMenu(null); }}
                  title="The same call as a grpcurl command line"
                >
                  Copy as grpcurl
                </button>
              </>
            )
          )}
          <button className="menu-item" onClick={() => { void copyToClipboard(menu.entry.endpoint); setMenu(null); }}>
            {menu.entry.kind === 'run' ? 'Copy path' : 'Copy endpoint'}
          </button>
          {source === 'browser' && (
            <button className="menu-item" onClick={() => { forgetHistory(menu.entry.id); setMenu(null); }}>
              Forget this call
            </button>
          )}
        </div>
      )}

      <CallRows
        entries={matched}
        now={now}
        peekId={peek?.entry.id ?? null}
        opened={opened}
        onBurst={toggle}
        onPeek={showPeek}
        onOpen={restoreHistory}
        onMenu={setMenu}
        onReplay={replay}
      />

      {peek && (
        <HistoryPeek
          panelRef={peekRef}
          entry={peek.entry}
          top={peek.top}
          onClose={() => setPeek(null)}
          onOpen={entry => { setPeek(null); restoreHistory(entry, { pin: true }); }}
          onReplay={entry => { setPeek(null); replay(entry); }}
        />
      )}
    </div>
  );
}

function CallRows({ entries, now, peekId, opened, onBurst, onPeek, onOpen, onMenu, onReplay }: {
  entries: HistoryEntry[];
  now: number;
  peekId: string | null;
  opened: Set<string>;
  onBurst: (id: string) => void;
  onPeek: (entry: HistoryEntry, at: HTMLElement) => void;
  onOpen: (entry: HistoryEntry, opts?: { pin?: boolean }) => void;
  onMenu: (menu: RowMenu) => void;
  onReplay: (entry: HistoryEntry) => void;
}) {
  const first = entries[0]?.id ?? null;
  return (
    <div className="history-calls" role="listbox" aria-label="Recorded calls">
      {groupByDay(entries, now).map(day => (
        <div key={day.key} role="group" aria-label={day.label}>
          <div className="history-day">{day.label}</div>
          {burstRepeats(day.entries, burstKey).map(burst => {
            const head = burst.entries[0];
            const repeats = burst.entries.length;
            if (repeats === 1) {
              return (
                <Row
                  key={head.id}
                  entry={head}
                  selected={peekId === head.id}
                  tabStop={peekId === head.id || (peekId === null && head.id === first)}
                  onPeek={onPeek}
                  onOpen={onOpen}
                  onMenu={onMenu}
                  onReplay={onReplay}
                />
              );
            }
            const took = tookRange(burst.entries.map(e => e.response.durationMs));
            const ok = !entryFailed(head);
            const burstOpen = opened.has(head.id);
            return (
              <div key={head.id}>
                <div
                  className={`row history-row is-call${burstOpen ? ' is-open' : ''}${ok ? '' : ' is-fail'}${peekId === head.id ? ' is-peeked' : ''}`}
                  role="option"
                  aria-selected={peekId === head.id}
                  tabIndex={peekId === head.id || (peekId === null && first === head.id) ? 0 : -1}
                  onClick={e => onPeek(head, e.currentTarget as HTMLElement)}
                  onDoubleClick={() => onOpen(head, { pin: true })}
                  onKeyDown={e => {
                    if (e.key === ' ') { e.preventDefault(); onPeek(head, e.currentTarget as HTMLElement); }
                    if (e.key === 'Enter') { e.preventDefault(); onOpen(head, { pin: true }); }
                    const step = treeStep(e.key);
                    if (step === null) return;
                    e.preventDefault();
                    moveRowFocus(e.currentTarget as HTMLElement, step, '.row.history-row');
                  }}
                  onContextMenu={e => { e.preventDefault(); onMenu({ x: e.clientX, y: e.clientY, entry: head }); }}
                  title={`${repeats} identical calls, ${stamp(burst.entries[repeats - 1].timestamp)}–${stamp(head.timestamp)}\nClick to see this one · ×${repeats} opens the rest`}
                >
                  <span className={`dot ${ok ? 'is-ok' : 'is-fail'}`} />
                  <span className="stack history-lines">
                    <CallLine entry={head} />
                    <RowMeta
                      entry={head}
                      took={took ?? ''}
                      repeats={repeats}
                      status={httpCode(head)}
                      burstOpen={burstOpen}
                      onBurst={() => onBurst(head.id)}
                    />
                  </span>
                  <RowActions entry={head} onMenu={onMenu} onReplay={onReplay} />
                </div>
                {burstOpen && burst.entries.map(entry => (
                  <Row
                    key={entry.id}
                    entry={entry}
                    nested
                      selected={peekId === entry.id}
                    onPeek={onPeek}
                    onOpen={onOpen}
                    onMenu={onMenu}
                    onReplay={onReplay}
                  />
                ))}
              </div>
            );
          })}
        </div>
      ))}
    </div>
  );
}

function Row({
  entry,
  nested,
  selected,
  onPeek,
  onOpen,
  onMenu,
  onReplay,
  tabStop = false,
}: {
  entry: HistoryEntry;
  nested?: boolean;
  selected?: boolean;
  tabStop?: boolean;
  onPeek: (entry: HistoryEntry, at: HTMLElement) => void;
  onOpen: (entry: HistoryEntry, opts?: { pin?: boolean }) => void;
  onMenu: (menu: RowMenu) => void;
  onReplay: (entry: HistoryEntry) => void;
}) {
  const ok = !entryFailed(entry);
  const here = useStore(callAddress);
  const mine = useStore(s => s.sessionId);
  const session = (entry as { session?: string }).session ?? null;
  const otherSession = session && session !== mine ? session.slice(0, 6) : null;
  const elsewhere = entry.connection && entry.connection.address !== here
    ? entry.connection.address
    : null;

  return (
    <div
      className={`row history-row is-call${nested ? ' is-nested' : ''}${ok ? '' : ' is-fail'}${selected ? ' is-peeked' : ''}`}
      role="option"
      aria-selected={!!selected}
      tabIndex={tabStop ? 0 : -1}
      onClick={e => onPeek(entry, e.currentTarget as HTMLElement)}
      onDoubleClick={() => onOpen(entry, { pin: true })}
      onKeyDown={e => {
        if (e.key === ' ') { e.preventDefault(); onPeek(entry, e.currentTarget as HTMLElement); }
        if (e.key === 'Enter') { e.preventDefault(); onOpen(entry, { pin: true }); }
        const step = treeStep(e.key);
        if (step === null) return;
        e.preventDefault();
        moveRowFocus(e.currentTarget as HTMLElement, step, '.row.history-row');
      }}
      onContextMenu={e => { e.preventDefault(); onMenu({ x: e.clientX, y: e.clientY, entry }); }}
      title={[
        entry.endpoint,
        `${stamp(entry.timestamp)} · ${ok ? 'ok' : 'failed'}`,
        entry.connection
          ? [
              entry.connection.address,
              entry.connection.protocol,
              entry.connection.tls ? 'tls' : '',
            ].filter(Boolean).join(' · ')
          : 'recorded before the connection was kept — opening it reproduces the request only',
        'Click to see it · double-click to open it in a tab',
      ].join('\n')}
    >
      <span className={`dot ${ok ? 'is-ok' : 'is-fail'}`} />
      <span className="stack history-lines">
        <CallLine entry={entry} />
        <RowMeta
          entry={entry}
          took={entry.response.durationMs != null ? durationLabel(entry.response.durationMs) : ''}
          where={elsewhere}
          elsewhen={otherSession}
          status={httpCode(entry)}
        />
      </span>
      <RowActions entry={entry} onMenu={onMenu} onReplay={onReplay} />
    </div>
  );
}

function httpCode(entry: HistoryEntry): number | null {
  if (!looksHttp(entry.endpoint)) return null;
  return entry.response.statusCode ?? null;
}

function CallLine({ entry }: { entry: HistoryEntry }) {
  const line = callSummary(entry);
  const shape = entry.kind === 'run' || looksHttp(entry.endpoint)
    ? null
    : shapeOfName(entry.response.shape);
  return (
    <span className={`mono grow history-payload is-${line.from}`} title={`${entry.endpoint}\n${line.text}`}>
      {shape && (
        <span className={`badge is-kind ${SHAPE_TONE[shape]}`} title={`${SHAPE_LABEL[shape]} — what the call resolved on the target`}>
          {SHAPE_LABEL[shape]}
        </span>
      )}
      <span className="history-method-name">{methodOf(entry.endpoint)}</span>
      {line.from === 'response' && <span className="history-back" aria-hidden="true">←</span>}
      {line.text}
    </span>
  );
}

function RowMeta({ entry, took, repeats, where, status, elsewhen, burstOpen, onBurst }: {
  entry: HistoryEntry;
  took: string;
  repeats?: number;
  where?: string | null;
  status?: number | null;
  elsewhen?: string | null;
  burstOpen?: boolean;
  onBurst?: () => void;
}) {
  const openFile = useStore(s => s.workspacePath);
  return (
    <span className="bar history-meta">
      <span className="mono" title={stamp(entry.timestamp)}>{clock(entry.timestamp)}</span>
      {status != null && (
        <span className={`mono history-status is-${httpStatusTone(status) ?? 'fail'}`} title={httpStatusLabel(status) ?? ''}>
          {status}
        </span>
      )}
      {entry.kind === 'run' && (
        <span className="badge" title="Recorded by a run of this file, not by a call made here">run</span>
      )}
      {entry.checks && (
        <span
          className={`badge${entry.checks.passed === entry.checks.total ? ' is-ok' : ' is-fail'}`}
          title={`${entry.checks.passed} of ${entry.checks.total} checks passed`}
        >
          {entry.checks.passed}/{entry.checks.total}
        </span>
      )}
      {entry.resolved && entry.resolved.length > 0 && (
        <span
          className="badge"
          title={`Sent with ${entry.resolved.join(', ')} resolved — the braces are what was typed, not what went out`}
        >
          {entry.resolved.length} resolved
        </span>
      )}
      {took && <span className="mono">{took}</span>}
      {repeats !== undefined && repeats > 1 && (
        onBurst
          ? (
            <button
              className={`mono history-burst${burstOpen ? ' is-on' : ''}`}
              onClick={e => { e.stopPropagation(); onBurst(); }}
              aria-expanded={burstOpen}
              title={burstOpen ? 'Hide the identical calls' : `Show the other ${repeats - 1}`}
            >
              ×{repeats}
            </button>
          )
          : <span className="mono">×{repeats}</span>
      )}
      {entry.kind !== 'run' && entry.collectionPath && entry.collectionPath !== openFile && (
        <span className="mono muted history-from" title={`Made from ${entry.collectionPath}`}>
          {entry.collectionPath.split('/').pop()}
        </span>
      )}
      {entry.datasetRow !== undefined && (
        <span className="mono muted" title="The DATASET row this call was made with">
          row {entry.datasetRow + 1}
        </span>
      )}
      {where && (
        <span className="mono history-where" title={`Went to ${where}`}>{shortTarget(where)}</span>
      )}
      {elsewhen && (
        <span className="mono muted" title={`Recorded by session ${elsewhen}, not by this one`}>
          {elsewhen}
        </span>
      )}
    </span>
  );
}

function RowActions({ entry, onMenu, onReplay }: {
  entry: HistoryEntry;
  onMenu: (menu: RowMenu) => void;
  onReplay: (entry: HistoryEntry) => void;
}) {
  return (
    <span className="history-acts">
      <button
        className="btn is-ghost is-icon is-sm"
        onClick={e => { e.stopPropagation(); onReplay(entry); }}
        title={entry.kind === 'run'
          ? `Run ${entry.collectionPath} again`
          : 'Send this call again, over the connection it was made on'}
        aria-label={entry.kind === 'run' ? 'Run again' : 'Send again'}
      >
        <Play size={11} />
      </button>
      <button
        className="btn is-ghost is-icon is-sm"
        onClick={e => {
          e.stopPropagation();
          const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
          onMenu({ x: Math.max(8, rect.right - 150), y: rect.bottom + 4, entry });
        }}
        title="What to do with this call"
        aria-label="More"
      >
        <MoreHorizontal size={12} />
      </button>
    </span>
  );
}
