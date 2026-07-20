import { useState, useEffect, useRef, useCallback } from 'react';
import { TopBar } from './TopBar';
import { StatusBar } from './StatusBar';
import { Sidebar } from '../collections/Sidebar';
import { HistoryPanel } from '../history/HistoryPanel';
import { ImportPanel } from '../collections/ImportPanel';
import { RequestPanel } from '../request/RequestPanel';
import { ResponsePanel } from '../response/ResponsePanel';
import { GctfInfoPanel } from '../request/GctfInfoPanel';
import { useStore } from '../../lib/store';
import { useDeepLink } from '../../lib/routing';
import { KeyboardShortcutHelp } from '../ui/KeyboardShortcutHelp';
import { SHORTCUT_DEFINITIONS, matchesHotkey, matchesDigitShortcut, isInputFocused } from '../../lib/hotkeys';
import { FolderOpen, Clock, Upload } from 'lucide-react';

type SidebarTab = 'collections' | 'history' | 'import';

const SIDEBAR_TABS: { key: SidebarTab; label: string; icon: React.ReactNode }[] = [
  { key: 'collections', label: 'Collections', icon: <FolderOpen size={13} /> },
  { key: 'history', label: 'History',    icon: <Clock size={13} /> },
  { key: 'import',   label: 'Import',    icon: <Upload size={13} /> },
];

export function PlayLayout() {
  const refreshCollections = useStore(s => s.refreshCollections);
  const loadStartupInfo = useStore(s => s.loadStartupInfo);
  const checkHealth = useStore(s => s.checkHealth);
  const collectionParsed = useStore(s => s.collectionParsed);
  const collectionsMtime = useStore(s => s.collectionsMtime);

  useEffect(() => { refreshCollections(); loadStartupInfo(); }, []);

  
  useEffect(() => {
    checkHealth();
    const interval = setInterval(checkHealth, 15000);
    return () => clearInterval(interval);
  }, []);

  
  useEffect(() => {
    let active = true;
    const poll = async () => {
      try {
        const res = await fetch('/api/info');
        if (!res.ok || !active) return;
        const data = await res.json();
        if (data.collections_mtime !== undefined && data.collections_mtime !== collectionsMtime) {
          refreshCollections();
          // Persist the new mtime so we don't refetch every tick forever.
          useStore.setState({ collectionsMtime: data.collections_mtime });
        }
      } catch {  }
    };
    const interval = setInterval(poll, 3000);
    return () => { active = false; clearInterval(interval); };
  }, [collectionsMtime, refreshCollections]);

  useDeepLink();

  const tabs = useStore(s => s.tabs);
  const activeTabId = useStore(s => s.activeTabId);
  const sidebarVisible = useStore(s => s.sidebarVisible);
  const showHotkeyHelp = useStore(s => s.showHotkeyHelp);
  const addTab = useStore(s => s.addTab);
  const removeTab = useStore(s => s.removeTab);
  const setActiveTab = useStore(s => s.setActiveTab);
  const execute = useStore(s => s.execute);
  const toggleSidebar = useStore(s => s.toggleSidebar);
  const setShowHotkeyHelp = useStore(s => s.setShowHotkeyHelp);
  const closeHotkeyHelp = useCallback(() => setShowHotkeyHelp(false), [setShowHotkeyHelp]);

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (isInputFocused()) return;

      // Alt+1-9: use e.code to work on any keyboard layout
      const digit = matchesDigitShortcut(e);
      if (digit) {
        e.preventDefault();
        const idx = parseInt(digit, 10) - 1;
        if (idx < tabs.length) setActiveTab(tabs[idx].id);
        return;
      }

      for (const def of SHORTCUT_DEFINITIONS) {
        if (!matchesHotkey(e, def)) continue;
        e.preventDefault();

        switch (def.key) {
          case 'Enter':
            execute();
            return;
          case 'w':
            if (activeTabId) removeTab(activeTabId);
            return;
          case 'T':
            addTab();
            return;
          case ']':
            if (tabs.length > 1 && activeTabId) {
              const idx = tabs.findIndex(t => t.id === activeTabId);
              const next = (idx + 1) % tabs.length;
              setActiveTab(tabs[next].id);
            }
            return;
          case '[':
            if (tabs.length > 1 && activeTabId) {
              const idx = tabs.findIndex(t => t.id === activeTabId);
              const prev = (idx - 1 + tabs.length) % tabs.length;
              setActiveTab(tabs[prev].id);
            }
            return;
          case 'b':
            toggleSidebar();
            return;
          case '?':
            setShowHotkeyHelp(true);
            return;
        }
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [tabs, activeTabId, addTab, removeTab, setActiveTab, execute, toggleSidebar, setShowHotkeyHelp]);

  const [sidebarW, setSidebarW] = useState(250);
  const [sidebarTab, setSidebarTab] = useState<SidebarTab>('collections');
  const dragRef = useRef<{ startX: number; startW: number } | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const onMouseDown = useCallback((e: React.MouseEvent) => {
    dragRef.current = { startX: e.clientX, startW: sidebarW };
    e.preventDefault();
  }, [sidebarW]);

  useEffect(() => {
    const mm = (e: MouseEvent) => {
      if (!dragRef.current) return;
      setSidebarW(Math.max(180, Math.min(500, dragRef.current.startW + e.clientX - dragRef.current.startX)));
    };
    const mu = () => { dragRef.current = null; };
    window.addEventListener('mousemove', mm);
    window.addEventListener('mouseup', mu);
    return () => { window.removeEventListener('mousemove', mm); window.removeEventListener('mouseup', mu); };
  }, []);

  return (
    <div style={{ height: '100%', display: 'flex', flexDirection: 'column' }}>
      <TopBar />
      <div ref={containerRef} style={{ flex: 1, display: 'flex', overflow: 'hidden' }}>
        {sidebarVisible && <>
        <aside style={{
          width: sidebarW, minWidth: 180, display: 'flex', flexDirection: 'column',
          background: 'var(--bg-secondary)',
        }}>
          <nav style={{ display: 'flex', borderBottom: '1px solid var(--border)' }}>
            {SIDEBAR_TABS.map(t => (
              <button
                key={t.key}
                onClick={() => setSidebarTab(t.key)}
                style={{
                  flex: 1, ...{ display: 'flex', alignItems: 'center', justifyContent: 'center' },
                  gap: 4, padding: '7px 4px', fontSize: 11, cursor: 'pointer',
                  border: 'none', background: 'none', transition: 'all 0.15s ease',
                  fontWeight: sidebarTab === t.key ? 600 : 400,
                  color: sidebarTab === t.key ? 'var(--accent)' : 'var(--text-secondary)',
                  borderBottom: sidebarTab === t.key ? '2px solid var(--accent)' : '2px solid transparent',
                }}
              >
                {t.icon} {t.label}
              </button>
            ))}
          </nav>

          <div style={{ flex: 1, overflow: 'auto' }}>
            {sidebarTab === 'collections' && <Sidebar />}
            {sidebarTab === 'history' && <HistoryPanel />}
            {sidebarTab === 'import' && <ImportPanel />}
          </div>
        </aside>

        <div
          onMouseDown={onMouseDown}
          style={{ width: 4, cursor: 'col-resize', background: 'var(--border)', flexShrink: 0 }}
        />
        </>}

        <main style={{ flex: 1, display: 'flex', flexDirection: 'column', overflow: 'hidden' }}>
          <div style={{ flex: 1, overflow: 'auto', padding: 12, display: 'flex', flexDirection: 'column', gap: 8 }}>
            <RequestPanel />
            <ResponsePanel />
            {collectionParsed && <GctfInfoPanel />}
          </div>
        </main>
      </div>
      <StatusBar />
      <KeyboardShortcutHelp open={showHotkeyHelp} onClose={closeHotkeyHelp} />
    </div>
  );
}
