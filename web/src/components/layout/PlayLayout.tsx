import { useState, useEffect, useMemo, useRef, useCallback, useSyncExternalStore } from 'react';
import { TopBar } from './TopBar';
import { StatusBar } from './StatusBar';
import { Sidebar } from '../collections/Sidebar';
import { RunControl } from '../collections/RunBar';
import { HistoryPanel } from '../history/HistoryPanel';
import { RequestPanel } from '../request/RequestPanel';
import { TabBar } from '../request/TabBar';
import { ChainRail } from '../request/ChainRail';
import { ResponsePanel } from '../response/ResponsePanel';
import { useStore } from '../../lib/store';
import { subscribeUnauthorized, tokenRejected } from '../../lib/play-token';
import { serverReachable, subscribeReach } from '../../lib/server-reach';
import { workspaceClass } from '../../lib/layout-state';
import { revealDelta } from '../../lib/reveal';
import { readNumber, writeText, readText } from 'luvo/data/storage';
import { useDeepLink, useUrlSync } from '../../lib/routing';
import { KeyboardShortcutHelp } from '../ui/KeyboardShortcutHelp';
import { useToast } from 'luvo/ui/useToast';
import { ConflictDialog } from '../ui/ConflictDialog';
import { Splitter } from 'luvo/ui/Splitter';
import { Tabs } from 'luvo/ui/Tabs';
import { tabPanelProps } from 'luvo/ui/tab-ids';
import { Drawer } from '../tools/Drawer';
import { FileDrop } from './FileDrop';
import { isChord, matchesHotkey, matchesDigitShortcut, isInputFocused, modalOpen, noteKeyDown, noteKeyUp } from 'luvo/input/hotkeys';
import type { ShiftTap } from 'luvo/input/hotkeys';
import { COMMANDS, SAY_TOAST, commandRefusal } from '../../lib/commands';
import { collapsesAt, nextStop, snap } from '../../lib/snap';
import type { CommandUi } from '../../lib/commands';
import { CommandPalette } from '../ui/CommandPalette';
import { FolderOpen, Clock, PanelLeftOpen, X } from 'lucide-react';
import { retheme } from '../../lib/monaco-theme';
import { mtimeMoved, pollsWhile, syncsAnyway } from '../../lib/poll-tick';

type SidebarTab = 'collections' | 'history';

const RAIL_STOPS = [200, 260, 340, 440] as const;
const ROW_STOPS = [280, 380, 480, 600] as const;
const COLUMN_STOPS = [40, 50, 60] as const;

const SIDEBAR_TABS: { key: SidebarTab; label: string; icon: React.ReactNode }[] = [
  { key: 'collections', label: 'Collections', icon: <FolderOpen size={13} /> },
  { key: 'history', label: 'History', icon: <Clock size={13} /> },
];

export function PlayLayout() {
  const toast = useToast();
  const refreshCollections = useStore(s => s.refreshCollections);
  const checkHealth = useStore(s => s.checkHealth);
  const collectionsMtime = useStore(s => s.collectionsMtime);
  const startupNote = useStore(s => s.startupNote);
  const buildMoved = useStore(s => s.buildMoved);
  const tokenIsStale = useSyncExternalStore(subscribeUnauthorized, tokenRejected, () => false);
  const serverIsThere = useSyncExternalStore(subscribeReach, serverReachable, () => true);
  useEffect(() => {
    if (serverIsThere) return;
    const id = setInterval(() => { void fetch('/api/health').catch(() => {}); }, 3000);
    return () => clearInterval(id);
  }, [serverIsThere]);

  useEffect(() => {
    const store = useStore.getState();
    store.refreshCollections();
    void store.loadStartupInfo();
    store.hydrateStaleTabs();
    void store.adoptRunningJob();
  }, []);

  const syncFiles = useCallback(async () => {
    await refreshCollections();
    const reloaded = await useStore.getState().syncOpenFiles();
    if (reloaded.length === 0) return;
    toast.info(reloaded.length === 1
      ? `${reloaded[0]} changed on disk — reloaded`
      : `${reloaded.length} open files changed on disk — reloaded`);
  }, [refreshCollections, toast]);

  useEffect(() => {
    checkHealth();
    const interval = setInterval(checkHealth, 15000);
    const onVisible = () => {
      if (document.visibilityState !== 'visible') return;
      checkHealth();
      void syncFiles();
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => {
      clearInterval(interval);
      document.removeEventListener('visibilitychange', onVisible);
    };
  }, [checkHealth, syncFiles]);

  const runRefused = useStore(s => s.runRefused);
  useEffect(() => {
    if (!runRefused) return;
    toast.refuse(runRefused.text);
  }, [runRefused, toast]);

  const serverHealthy = useStore(s => s.serverHealthy);
  const wasHealthy = useRef(serverHealthy);
  useEffect(() => {
    if (wasHealthy.current && !serverHealthy) {
      toast.error('The workbench is not answering — nothing can be read or saved until it is back');
    }
    if (!wasHealthy.current && serverHealthy) {
      toast.success('The workbench is answering again');
    }
    wasHealthy.current = serverHealthy;
  }, [serverHealthy, toast]);

  const ticks = useRef(0);
  useEffect(() => {
    let active = true;
    const poll = async () => {
      if (!pollsWhile(document.visibilityState)) return;
      const tick = ++ticks.current;
      try {
        const res = await fetch('/api/info');
        if (!res.ok || !active) return;
        const data = await res.json();
        if (mtimeMoved(collectionsMtime, data.collections_mtime)) {
          useStore.setState({ collectionsMtime: data.collections_mtime });
          await syncFiles();
          return;
        }
        if (!syncsAnyway(tick) || !active) return;
        const reloaded = await useStore.getState().syncOpenFiles();
        if (reloaded.length > 0) {
          toast.info(reloaded.length === 1
            ? `${reloaded[0]} changed on disk — reloaded`
            : `${reloaded.length} open files changed on disk — reloaded`);
        }
      } catch {  }
    };
    const interval = setInterval(poll, 3000);
    return () => { active = false; clearInterval(interval); };
  }, [collectionsMtime, syncFiles, toast]);

  useDeepLink();
  useUrlSync();

  const tabs = useStore(s => s.tabs);
  const sidebarVisible = useStore(s => s.sidebarVisible);
  const toggleSidebar = useStore(s => s.toggleSidebar);
  const showHotkeyHelp = useStore(s => s.showHotkeyHelp);
  const setActiveTab = useStore(s => s.setActiveTab);
  const setShowHotkeyHelp = useStore(s => s.setShowHotkeyHelp);
  const closeHotkeyHelp = useCallback(() => setShowHotkeyHelp(false), [setShowHotkeyHelp]);

  const [paletteOpen, setPaletteOpen] = useState(false);

  const ui: CommandUi = useMemo(() => ({
    openPalette: () => setPaletteOpen(true),
    closePalette: () => setPaletteOpen(false),
    openHelp: () => setShowHotkeyHelp(true),
    saveFile: () => useStore.getState().requestSave(),
    openImport: () => useStore.getState().requestImport(),
    say: (kind, message) => toast[SAY_TOAST[kind]](message),
  }), [setShowHotkeyHelp, toast]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (modalOpen()) return;
      const typing = isInputFocused();

      const digit = matchesDigitShortcut(e);
      if (digit) {
        e.preventDefault();
        const idx = parseInt(digit, 10) - 1;
        if (idx < tabs.length) setActiveTab(tabs[idx].id);
        return;
      }

      for (const command of COMMANDS) {
        if (!command.hotkey) continue;
        if (typing && !isChord(command.hotkey)) continue;
        if (!matchesHotkey(e, { ...command.hotkey, category: 'general', description: '' })) continue;
        e.preventDefault();
        const state = useStore.getState();
        const refused = commandRefusal(command, state);
        if (refused !== null) { ui.say('note', refused); return; }
        command.run(state, ui);
        return;
      }
    };
    window.addEventListener('keydown', handler, true);
    return () => window.removeEventListener('keydown', handler, true);
  }, [tabs, setActiveTab, ui]);

  useEffect(() => {
    let taps: ShiftTap = { lastUpAt: null };
    const down = (e: KeyboardEvent) => { taps = noteKeyDown(taps, e.key); };
    const up = (e: KeyboardEvent) => {
      const { state, fired } = noteKeyUp(taps, e.key, performance.now());
      taps = state;
      if (!fired) return;
      setPaletteOpen(open => {
        if (open) return false;
        return !modalOpen();
      });
    };
    window.addEventListener('keydown', down, true);
    window.addEventListener('keyup', up, true);
    return () => {
      window.removeEventListener('keydown', down, true);
      window.removeEventListener('keyup', up, true);
    };
  }, []);

  const layout = useStore(s => s.layout);
  const [sidebarW, setSidebarW] = useState(() => readNumber('play.sidebarW', 260, 180, 500));
  const requestTab = useStore(s => s.requestTab);
  const hasOutcome = useStore(s =>
    s.response !== null || (s.run.kind === 'bench' && (s.run.benchReport !== null || s.run.benchProgress !== null)));
  const [requestH, setRequestH] = useState(() => readNumber('play.requestH', 380, 220, 900));
  const [requestSized, setRequestSized] = useState(() => readText('play.requestSized') === 'on');
  const sizeRequest = useCallback((next: number) => {
    setRequestH(next);
    setRequestSized(true);
    writeText('play.requestH', String(next));
    writeText('play.requestSized', 'on');
  }, []);
  const [requestPct, setRequestPct] = useState(() => readNumber('play.requestPct', 50, 20, 80));
  const splitDrag = useRef<{ y: number; h: number } | null>(null);
  const colDrag = useRef<{ x: number; pct: number; width: number } | null>(null);
  const workspaceRef = useRef<HTMLDivElement>(null);
  const outcomeRef = useRef<HTMLDivElement>(null);

  const response = useStore(s => s.response);
  useEffect(() => {
    if (!response) return;
    const pane = outcomeRef.current;
    if (!pane) return;
    const scroller = pane.closest('.main-scroll');
    if (!scroller) return;

    const reveal = () => {
      const box = pane.getBoundingClientRect();
      const view = scroller.getBoundingClientRect();
      const delta = revealDelta(box.top - view.top, box.height, view.height);
      if (delta === 0) return;
      scroller.scrollBy({
        top: delta,
        behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth',
      });
    };

    reveal();
    const watch = new ResizeObserver(() => reveal());
    watch.observe(pane);
    const stop = window.setTimeout(() => watch.disconnect(), 1000);
    return () => { watch.disconnect(); window.clearTimeout(stop); };
  }, [response]);

  useEffect(() => {
    const move = (e: MouseEvent) => {
      if (splitDrag.current) {
        const raw = splitDrag.current.h + (e.clientY - splitDrag.current.y);
        setRequestH(snap(Math.min(900, Math.max(220, raw)), ROW_STOPS, 12, !e.altKey));
      }
      if (colDrag.current) {
        const delta = ((e.clientX - colDrag.current.x) / colDrag.current.width) * 100;
        const raw = Math.min(75, Math.max(25, colDrag.current.pct + delta));
        setRequestPct(snap(raw, COLUMN_STOPS, 2, !e.altKey));
      }
    };
    const up = () => {
      if (splitDrag.current) {
        splitDrag.current = null;
        setRequestH(h => { sizeRequest(h); return h; });
      }
      if (colDrag.current) {
        colDrag.current = null;
        setRequestPct(p => { writeText('play.requestPct', String(p)); return p; });
      }
      document.body.style.userSelect = '';
    };
    document.addEventListener('mousemove', move);
    document.addEventListener('mouseup', up);
    return () => { document.removeEventListener('mousemove', move); document.removeEventListener('mouseup', up); };
  }, [sizeRequest]);

  const themeMode = useStore(s => s.themeMode);
  const palette = useStore(s => s.palette);
  useEffect(() => { retheme(themeMode); }, [themeMode, palette]);

  const sidebarTab = useStore(s => s.sidebarTab);
  const setSidebarTab = useStore(s => s.showSidebarTab);
  const dragRef = useRef<{ startX: number; startW: number } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    dragRef.current = { startX: e.clientX, startW: sidebarW };
    e.preventDefault();
  }, [sidebarW]);

  useEffect(() => {
    const mm = (e: MouseEvent) => {
      if (!dragRef.current) return;
      const raw = dragRef.current.startW + e.clientX - dragRef.current.startX;
      if (collapsesAt(raw, RAIL_STOPS)) {
        if (useStore.getState().sidebarVisible) useStore.getState().toggleSidebar();
        return;
      }
      setSidebarW(snap(Math.max(180, Math.min(500, raw)), RAIL_STOPS, 12, !e.altKey));
    };
    const mu = () => {
      if (dragRef.current) setSidebarW(w => { writeText('play.sidebarW', String(w)); return w; });
      dragRef.current = null;
    };
    window.addEventListener('mousemove', mm);
    window.addEventListener('mouseup', mu);
    return () => { window.removeEventListener('mousemove', mm); window.removeEventListener('mouseup', mu); };
  }, []);

  return (
    <div className="app">
      <TopBar />
      {!serverIsThere && (
        <div className="note is-warn startup-note">
          <span className="grow">
            The workbench is not answering — it was stopped, or something else took its port. Nothing
            is lost: the tabs are here and the files are on disk.
          </span>
          <button className="btn is-sm is-ghost" onClick={() => window.location.reload()}>reload</button>
        </div>
      )}
      {buildMoved && (
        <div className="note startup-note">
          <span className="grow">
            The workbench has been updated — this tab is still running the build it opened with.
            Reloading picks up the new one; the tabs come back with it.
          </span>
          <button className="btn is-sm is-ghost" onClick={() => window.location.reload()}>reload</button>
        </div>
      )}
      {tokenIsStale && (
        <div className="note is-warn startup-note">
          <span className="grow">
            This workbench no longer accepts the token this page has — it prints a new one each time
            it starts. Open the link it printed, or set <span className="mono">GRPCTESTIFY_PLAY_TOKEN</span> so
            it keeps one.
          </span>
          <button className="btn is-sm is-ghost" onClick={() => window.location.reload()}>
            reload
          </button>
        </div>
      )}
      {startupNote && (
        <div className="note is-warn startup-note">
          <span className="grow">{startupNote}</span>
          <button
            className="btn is-ghost is-icon is-sm"
            aria-label="Dismiss"
            onClick={() => useStore.getState().dismissStartupNote()}
          >
            <X size={11} />
          </button>
        </div>
      )}
      <div ref={containerRef} className="body">
        {!sidebarVisible && (
          <button
            className="rail-stub"
            onClick={toggleSidebar}
            title="Show collections (⌘B)"
            aria-label="Show collections"
          >
            <PanelLeftOpen size={13} />
          </button>
        )}

        {sidebarVisible && <>
        <aside className="sidebar" style={{ width: sidebarW }}>
          <Tabs
            id="rail"
            label="The rail — files or history"
            items={SIDEBAR_TABS.map(t => ({ key: t.key, label: <>{t.icon} {t.label}</> }))}
            value={sidebarTab}
            onChange={setSidebarTab}
          >
            <span className="grow" />
            {sidebarTab === 'collections' && <RunControl />}
          </Tabs>

          <div className="sidebar-body" {...tabPanelProps('rail', sidebarTab)}>
            {sidebarTab === 'collections' && <Sidebar />}
            {sidebarTab === 'history' && <HistoryPanel />}
          </div>
        </aside>

        <Splitter
          className="split"
          orientation="vertical"
          label="Collections width"
          title="Drag to resize · arrows to nudge"
          value={sidebarW}
          min={180}
          max={500}
          onValue={next => { setSidebarW(next); writeText('play.sidebarW', String(next)); }}
          onMouseDown={onMouseDown}
        />
        </>}

        <main>
          <TabBar />
          <div className="main-scroll">
            <ChainRail />
            <div ref={workspaceRef} className={workspaceClass(layout, requestTab, hasOutcome, requestSized)}>
              <div
                className="request-pane"
                style={layout === 'columns'
                  ? { width: `${requestPct}%` }
                  : ({ ['--editor-h' as string]: `${requestH}px` } as React.CSSProperties)}
              >
                <RequestPanel />
              </div>
              <Splitter
                className={layout === 'columns' ? 'split' : 'hsplit'}
                orientation={layout === 'columns' ? 'vertical' : 'horizontal'}
                label={layout === 'columns' ? 'Request pane width' : 'Request pane height'}
                title={layout === 'columns'
                  ? 'Drag to resize · double-click for the next stop · ⌥ to ignore the stops · arrows to nudge'
                  : 'Drag to resize · double-click for the next stop · arrows to nudge'}
                value={layout === 'columns' ? requestPct : requestH}
                min={layout === 'columns' ? 20 : 220}
                max={layout === 'columns' ? 80 : 900}
                step={layout === 'columns' ? 2 : 24}
                onValue={next => {
                  if (layout === 'columns') { setRequestPct(next); writeText('play.requestPct', String(next)); }
                  else sizeRequest(next);
                }}
                onDoubleClick={() => layout === 'columns'
                  ? setRequestPct(p => nextStop(p, COLUMN_STOPS))
                  : setRequestH(h => { const next = nextStop(h, ROW_STOPS); sizeRequest(next); return next; })}
                onMouseDown={e => {
                  document.body.style.userSelect = 'none';
                  if (layout === 'columns') {
                    colDrag.current = { x: e.clientX, pct: requestPct, width: workspaceRef.current?.clientWidth || 1 };
                  } else {
                    splitDrag.current = { y: e.clientY, h: requestH };
                  }
                }}
              />
              <div className="response-pane" ref={outcomeRef}>
                <ResponsePanel />
              </div>
            </div>
          </div>
        </main>
      </div>
      <FileDrop />
      <Drawer />
      <StatusBar />
      <CommandPalette open={paletteOpen} onClose={() => setPaletteOpen(false)} ui={ui} />
      <KeyboardShortcutHelp open={showHotkeyHelp} onClose={closeHotkeyHelp} />
      <ConflictDialog />
    </div>
  );
}
