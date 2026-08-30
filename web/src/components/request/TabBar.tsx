import { useState, useEffect, useMemo, useRef, useCallback } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { COLUMNS_FIT } from '../../lib/layout-state';
import { useStore, copyNote, isTabDirty, isActiveTabDirty, callAddress, effectiveTls, listedPaths, tabFileMissing } from '../../lib/store';
import { useShallow } from 'zustand/react/shallow';
import { isSecretHeader } from '../../lib/secret-headers';
import { bodyWarnings } from '../../lib/share-notice';
import { findVariables } from '../../lib/env';
import { byteSize, humanBytes, shortPath } from '../../lib/format';
import { ImportPanel } from '../collections/ImportPanel';
import type { Tab } from '../../lib/types';
import { tabTitle, titleIsBorrowed } from '../../lib/tab-title';
import { tabAtStake } from '../../lib/preview-slot';
import { encodeCollectionLink } from '../../lib/deeplink';
import { hasToken } from '../../lib/play-token';
import { dropIndex } from 'luvo/input/tab-keys';
import { ContextMenu } from 'luvo/ui/ContextMenu';
import { copyToClipboard } from 'luvo/data/clipboard';
import { Plus, X, XCircle, FileSymlink, Pencil, ArrowRightToLine, Search, Share2, Terminal, ChevronDown, ChevronLeft, ChevronRight, MoreHorizontal, Columns2, Rows2, Pin, Copy, FileX } from 'lucide-react';
import { useDismiss } from 'luvo/input/useDismiss';
import { Popover } from 'luvo/ui/Popover';
import { isHttpRequest } from '../../lib/http-endpoint';
import { useModal } from 'luvo/ui/ModalContext';
import { useToast } from 'luvo/ui/ToastContext';
import { count } from 'luvo/data/plural';
import { scrollForActive } from '../../lib/tab-scroll';
import { closeWithGate, type CloseChoice } from '../../lib/close-gate';

interface CtxMenu {
  x: number;
  y: number;
  tabId: string;
}

export function TabBar() {
  const tabs = useStore(s => s.tabs);
  const activeTabId = useStore(s => s.activeTabId);
  const liveEndpoint = useStore(s => s.request.endpoint);
  const nameOf = useCallback(
    (tab: Tab) => tabTitle(tab, tab.id === activeTabId ? liveEndpoint : undefined),
    [activeTabId, liveEndpoint],
  );
  const setActiveTab = useStore(s => s.setActiveTab);
  const removeTab = useStore(s => s.removeTab);
  const moveTab = useStore(s => s.moveTab);
  const requestSaveAs = useStore(s => s.requestSaveAs);
  const pinTab = useStore(s => s.pinTab);
  const addTab = useStore(s => s.addTab);
  const setTabLabel = useStore(s => s.setTabLabel);
  const request = useStore(s => s.request);
  const workspaceOriginal = useStore(s => s.workspaceOriginal);
  const collectionParsed = useStore(s => s.collectionParsed);
  const address = useStore(s => s.address);
  const addressTouched = useStore(s => s.addressTouched);
  const protocol = useStore(s => s.protocol);
  const protocolTouched = useStore(s => s.protocolTouched);
  const rawContent = useStore(s => s.rawContent);
  const rawOriginal = useStore(s => s.rawOriginal);
  const getGrpcurlCommand = useStore(s => s.getGrpcurlCommand);
  const isExecuting = useStore(s => s.response?.status) === 'pending';
  const modal = useModal();
  const toast = useToast();

  const onDisk = useStore(listedPaths);
  const tabDirty = useCallback((tab: Tab): boolean => {
    if (tab.id !== activeTabId) return isTabDirty(tab);
    return isActiveTabDirty(tab, { request, rawContent, rawOriginal, workspaceOriginal, collectionParsed, address, addressTouched, protocol, protocolTouched });
  }, [activeTabId, request, rawContent, rawOriginal, workspaceOriginal, collectionParsed, address, addressTouched, protocol, protocolTouched]);

  const requestClose = useCallback(async (tabId: string) => {
    const tab = tabs.find(t => t.id === tabId);
    if (!tab || !tabDirty(tab)) { removeTab(tabId); return; }
    setActiveTab(tabId);
    const answer = await modal.choose(
      `${tabTitle(tab)} has unsaved changes`,
      'Close it anyway?',
      [
        { label: 'discard', value: 'discard', tone: 'danger' },
        { label: 'save & close', value: 'save', tone: 'primary' },
      ],
    );
    let failed: string | null = null;
    await closeWithGate(answer as CloseChoice, { hasPath: !!tab.collectionPath }, {
      close: () => removeTab(tabId),
      save: async () => {
        try {
          return await useStore.getState().saveWorkspace();
        } catch (err: any) {
          failed = err?.message || 'Save failed';
          return false;
        }
      },
      nameIt: requestSaveAs,
    });
    if (failed) toast.error(failed);
  }, [tabs, tabDirty, removeTab, setActiveTab, modal, requestSaveAs, toast]);

  const closeIntent = useStore(s => s.closeIntent);
  const closeAllIntent = useStore(s => s.closeAllIntent);
  const tabListIntent = useStore(s => s.tabListIntent);
  const closeRef = useRef(requestClose);
  useEffect(() => { closeRef.current = requestClose; }, [requestClose]);
  useEffect(() => {
    if (closeIntent === 0) return;
    const id = useStore.getState().activeTabId;
    if (id) void closeRef.current(id);
  }, [closeIntent]);

  const [showList, setShowList] = useState(false);
  const [listFilter, setListFilter] = useState('');
  const [cursor, setCursor] = useState(0);
  const listRef = useDismiss<HTMLDivElement>(showList, useCallback(() => setShowList(false), []));
  const listed = useMemo(() => {
    const needle = listFilter.trim().toLowerCase();
    if (needle === '') return tabs;
    return tabs.filter(t =>
      nameOf(t).toLowerCase().includes(needle)
      || (t.collectionPath ?? '').toLowerCase().includes(needle));
  }, [tabs, listFilter, nameOf]);
  useEffect(() => { if (!showList) setListFilter(''); }, [showList]);
  useEffect(() => { setCursor(0); }, [listFilter, showList]);

  useEffect(() => { if (tabListIntent > 0) setShowList(true); }, [tabListIntent]);

  const closeAllRef = useRef<() => void>(() => {});
  useEffect(() => {
    if (closeAllIntent === 0) return;
    closeAllRef.current();
  }, [closeAllIntent]);

  const isHttp = useStore(s => isHttpRequest(s.workspacePath, s.request.endpoint));
  const dialled = useStore(callAddress);
  const { tls, tlsInsecure } = useStore(useShallow(effectiveTls));
  const getCurlCommand = useStore(s => s.getCurlCommand);
  const withEnv = useStore(copyNote);
  const handleCopyCurl = useCallback(async () => {
    try {
      await copyToClipboard(getCurlCommand());
      toast.success(`curl command copied!${withEnv}`);
    } catch {
      toast.error('The browser refused the clipboard');
    }
  }, [getCurlCommand, toast, withEnv]);

  const handleCopyGrpcurl = useCallback(async () => {
    try {
      const cmd = await getGrpcurlCommand();
      await copyToClipboard(cmd);
      toast.success(`grpcurl command copied!${withEnv}`);
    } catch (err: any) {
      toast.error(err?.message || 'Failed to build grpcurl command');
    }
  }, [getGrpcurlCommand, toast, withEnv]);

  const scrollRef = useRef<HTMLDivElement>(null);
  const activeRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const tab = activeRef.current;
    const view = scrollRef.current;
    if (!tab || !view) return;
    const gutter = Number.parseFloat(getComputedStyle(view).paddingLeft) || 0;
    const gutterEnd = Number.parseFloat(getComputedStyle(view).paddingRight) || 0;
    const next = scrollForActive(
      { scrollLeft: view.scrollLeft, width: view.clientWidth, padStart: gutter, padEnd: gutterEnd },
      { left: tab.offsetLeft, width: tab.offsetWidth },
    );
    if (next !== null) view.scrollLeft = next;
  }, [activeTabId, tabs.length]);
  const activeLabel = tabs.find(t => t.id === activeTabId)?.label ?? 'this request';
  const [showImport, setShowImport] = useState(false);
  const importIntent = useStore(s => s.importIntent);
  useEffect(() => { if (importIntent > 0) setShowImport(true); }, [importIntent]);
  const [canScrollLeft, setCanScrollLeft] = useState(false);
  const [canScrollRight, setCanScrollRight] = useState(false);

  const updateScrollState = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    setCanScrollLeft(el.scrollLeft > 0);
    setCanScrollRight(el.scrollLeft + el.clientWidth < el.scrollWidth - 1);
  }, []);

  useEffect(() => {
    updateScrollState();
    const el = scrollRef.current;
    if (!el) return;
    let pending = 0;
    let frame = 0;
    const wheel = (e: WheelEvent) => {
      if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
      if (e.deltaY === 0) return;
      pending += e.deltaY;
      if (frame !== 0) return;
      frame = requestAnimationFrame(() => {
        el.scrollLeft += pending;
        pending = 0;
        frame = 0;
      });
    };
    el.addEventListener('wheel', wheel, { passive: true });
    el.addEventListener('scroll', updateScrollState, { passive: true });
    const ro = new ResizeObserver(updateScrollState);
    ro.observe(el);
    return () => {
      if (frame !== 0) cancelAnimationFrame(frame);
      el.removeEventListener('wheel', wheel);
      el.removeEventListener('scroll', updateScrollState);
      ro.disconnect();
    };
  }, [tabs.length, updateScrollState]);

  const scrollBy = useCallback((dir: number) => {
    const el = scrollRef.current;
    if (!el) return;
    const page = Math.max(160, el.clientWidth - 180);
    el.scrollBy({ left: dir * page, behavior: 'smooth' });
  }, []);

  const [ctxMenu, setCtxMenu] = useState<CtxMenu | null>(null);
  const [showMore, setShowMore] = useState(false);
  const layout = useStore(s => s.layout);
  const setLayout = useStore(s => s.setLayout);
  const [wideEnough, setWideEnough] = useState(() =>
    typeof window === 'undefined' || window.matchMedia(COLUMNS_FIT).matches);
  useEffect(() => {
    const query = window.matchMedia(COLUMNS_FIT);
    const read = () => setWideEnough(query.matches);
    read();
    query.addEventListener('change', read);
    return () => query.removeEventListener('change', read);
  }, []);
  const moreRef = useDismiss<HTMLDivElement>(showMore, useCallback(() => setShowMore(false), []));
  const share = useStore(s => s.share);
  const activeHasFile = useStore(s => s.workspacePath !== null);
  const startShare = useStore(s => s.startShare);
  const closeShare = useStore(s => s.closeShare);
  const shareCreated = useStore(s => s.shareCreated);
  const toggleShareHeader = useStore(s => s.toggleShareHeader);
  const setShareTtl = useStore(s => s.setShareTtl);
  const [sharing, setSharing] = useState(false);
  const closeMenu = useCallback(() => setCtxMenu(null), []);

  const handleContextMenu = useCallback((e: React.MouseEvent, tabId: string) => {
    e.preventDefault();
    setCtxMenu({ x: e.clientX, y: e.clientY, tabId });
  }, []);

  const handleRename = useCallback(async (tabId: string) => {
    const tab = tabs.find(t => t.id === tabId);
    if (!tab) return;
    const name = await modal.prompt('Rename tab', 'Tab name:', tabTitle(tab));
    if (name) setTabLabel(tabId, name);
    closeMenu();
  }, [tabs, modal, setTabLabel, closeMenu]);

  const handleDuplicate = useCallback((tabId: string) => {
    const tab = tabs.find(t => t.id === tabId);
    if (!tab) return;
    if (!addTab({ endpoint: tab.endpoint, headers: tab.headers, bodies: tab.bodies })) {
      toast.error('Every open tab has unsaved edits — close one first');
    }
    closeMenu();
  }, [tabs, addTab, closeMenu, toast]);

  const closeMany = useCallback(async (targets: Tab[]) => {
    closeMenu();
    const losing = targets.filter(t => tabAtStake(t, tabDirty(t)));
    if (losing.length > 0) {
      const ok = await modal.confirm(
        losing.length === 1
          ? `${tabTitle(losing[0])} holds work that is not on disk`
          : `${losing.length} tabs hold work that is not on disk`,
        'Closing them discards it.',
        { confirmText: 'discard', cancelText: 'cancel', danger: true },
      );
      if (!ok) return;
    }
    for (const t of targets) removeTab(t.id);
  }, [tabDirty, removeTab, closeMenu, modal]);

  const handleCloseOthers = useCallback((tabId: string) => {
    void closeMany(tabs.filter(t => t.id !== tabId));
  }, [tabs, closeMany]);

  const handleCloseAll = useCallback(() => {
    void closeMany(tabs);
  }, [tabs, closeMany]);

  useEffect(() => { closeAllRef.current = handleCloseAll; }, [handleCloseAll]);

  const goneTabs = useMemo(() => tabs.filter(t => tabFileMissing(t, onDisk)), [tabs, onDisk]);
  const handleCloseGone = useCallback(() => {
    void closeMany(goneTabs);
  }, [goneTabs, closeMany]);

  const handleCloseRight = useCallback((tabId: string) => {
    const idx = tabs.findIndex(t => t.id === tabId);
    if (idx === -1) return;
    void closeMany(tabs.slice(idx + 1));
  }, [tabs, closeMany]);

  const handleShare = useCallback(async () => {
    const tab = tabs.find(t => t.id === activeTabId);
    if (startShare() !== 'link' || !tab?.collectionPath) return;
    try {
      await copyToClipboard(`${window.location.origin}${encodeCollectionLink(tab.collectionPath)}`);
      toast.success(`Link to ${tab.collectionPath} copied — it opens the file here`);
    } catch {
      toast.error('The browser refused the clipboard');
    }
  }, [tabs, activeTabId, startShare, toast]);

  const handleCreateShare = useCallback(async () => {
    const tab = tabs.find(t => t.id === activeTabId);
    if (!tab) return;
    setSharing(true);

    const filteredHeaders: Record<string, string> = {};
    for (const [key, val] of Object.entries(request.headers)) {
      if (share?.headers[key] !== false) {
        filteredHeaders[key] = val;
      }
    }
    const includeSecrets = Object.keys(filteredHeaders).some(isSecretHeader);
    const omitted = Object.keys(request.headers).filter(key => share?.headers[key] === false);

    try {
      const res = await fetch('/api/share', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          endpoint: request.endpoint,
          headers: Object.keys(filteredHeaders).length > 0 ? filteredHeaders : undefined,
          bodies: request.bodies,
          address: dialled || undefined,
          ...(isHttp ? {} : { protocol, tls, tls_insecure: tls ? tlsInsecure : undefined }),
          ttl_days: share?.ttl ?? 7,
          include_secrets: includeSecrets,
          omitted,
        }),
      });
      if (!res.ok) { toast.error('Failed to create share'); return; }
      const data = await res.json();
      const link = `${window.location.origin}${data.url}`;
      const expires = new Date(data.expires_at).toLocaleDateString();
      shareCreated(link, expires);
      try {
        await copyToClipboard(link);
        toast.success(`Link copied! Expires ${expires}`);
      } catch {
        toast.error('The browser refused the clipboard — the link is in the dialog');
      }
    } catch {
      toast.error('Failed to create share');
    } finally {
      setSharing(false);
    }
  }, [tabs, activeTabId, request, share, shareCreated, toast, dialled, protocol, tls, tlsInsecure, isHttp]);

  const [drag, setDrag] = useState<{ from: number; to: number } | null>(null);
  const dragFrom = useRef<number | null>(null);

  const dropOn = (index: number, e: React.DragEvent<HTMLDivElement>) => {
    const box = e.currentTarget.getBoundingClientRect();
    return dropIndex(index, e.clientX > box.left + box.width / 2);
  };

  if (!tabs || tabs.length === 0) return null;

  return (
    <div className="tabs tab-strip">
      <div className={`tab-scroller${canScrollLeft ? ' has-left' : ''}${canScrollRight ? ' has-right' : ''}`}>
        <div ref={scrollRef} className="tabs-scroll" role="tablist" aria-label="Open files">
          {tabs.map((tab, index) => {
            const isActive = tab.id === activeTabId;
            return (
              <div
                key={tab.id}
                ref={isActive ? activeRef : undefined}
                role="tab"
                aria-selected={isActive}
                tabIndex={isActive ? 0 : -1}
                className={[
                  'tab tab-row',
                  isActive ? 'is-on' : '',
                  tab.isPreview ? 'is-preview' : '',
                  drag?.from === index ? 'is-dragging' : '',
                  drag && drag.to === index ? 'is-drop-before' : '',
                  drag && drag.to === tabs.length && index === tabs.length - 1 ? 'is-drop-after' : '',
                  tabFileMissing(tab, onDisk) ? 'is-gone' : '',
                ].filter(Boolean).join(' ')}
                draggable
                onDragStart={e => {
                  e.dataTransfer.effectAllowed = 'move';
                  e.dataTransfer.setData('text/plain', tab.id);
                  dragFrom.current = index;
                  setDrag({ from: index, to: index });
                }}
                onDragOver={e => {
                  const from = dragFrom.current;
                  if (from === null) return;
                  e.preventDefault();
                  e.dataTransfer.dropEffect = 'move';
                  const to = dropOn(index, e);
                  setDrag(d => (d?.to === to ? d : { from, to }));
                }}
                onDrop={e => {
                  const from = dragFrom.current;
                  if (from === null) return;
                  e.preventDefault();
                  const to = dropOn(index, e);
                  moveTab(from, to > from ? to - 1 : to);
                  dragFrom.current = null;
                  setDrag(null);
                }}
                onDragEnd={() => { dragFrom.current = null; setDrag(null); }}
                onClick={() => setActiveTab(tab.id)}
                onDoubleClick={() => pinTab(tab.id)}
                title={tabFileMissing(tab, onDisk)
                  ? `${tab.collectionPath} is not on disk any more — this tab still holds it, and Save writes it back`
                  : tab.isPreview ? `${nameOf(tab)} — preview, Enter or double-click keeps it`
                  : titleIsBorrowed(tab, tab.id === activeTabId ? liveEndpoint : undefined)
                    ? `${nameOf(tab)} — not saved yet, so this is what it holds rather than a name`
                    : nameOf(tab)}
                onContextMenu={e => handleContextMenu(e, tab.id)}
                onKeyDown={e => stepTabs(e, tabs, index, {
                  select: setActiveTab,
                  pin: pinTab,
                  close: id => void requestClose(id),
                  menu: (id, el) => {
                    const rect = el.getBoundingClientRect();
                    setCtxMenu({ x: rect.left, y: rect.bottom + 4, tabId: id });
                  },
                })}
              >
                <span className="tab-label">{nameOf(tab)}</span>
                <span className="tab-slot">
                  {tabDirty(tab) && <span className="tab-dirty" title="Unsaved changes" />}
                  <button
                    className="tab-close"
                    aria-label={`Close ${nameOf(tab)}`}
                    onClick={e => { e.stopPropagation(); void requestClose(tab.id); }}
                  >
                    <X size={12} />
                  </button>
                </span>
              </div>
            );
          })}
        </div>

        {canScrollLeft && (
          <button className="tab-scroll-btn is-left" onClick={() => scrollBy(-1)} aria-label="Scroll tabs left">
            <ChevronLeft size={14} />
          </button>
        )}
        {canScrollRight && (
          <button className="tab-scroll-btn is-right" onClick={() => scrollBy(1)} aria-label="Scroll tabs right">
            <ChevronRight size={14} />
          </button>
        )}
      </div>

      {tabs.length > 1 && (
        <div ref={listRef} className="picker">
          <button
            className="btn is-ghost tab-list-btn"
            onClick={() => setShowList(v => !v)}
            aria-haspopup="menu"
            aria-expanded={showList}
            title={`All ${tabs.length} open tabs`}
          >
            <ChevronDown size={12} />
            <span className="mono">{tabs.length}</span>
          </button>
          <Popover open={showList} anchor={listRef} align="end">
            <div className="menu tab-list">
              <div className="field-frame">
                <Search size={12} className="muted inset-start no-shrink" />
                <input
                  className="field"
                  role="combobox"
                  aria-expanded
                  aria-controls="tab-list-options"
                  value={listFilter}
                  autoFocus
                  onChange={e => setListFilter(e.target.value)}
                  aria-activedescendant={listed[cursor] ? `tab-option-${listed[cursor].id}` : undefined}
                  onKeyDown={e => {
                    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
                      e.preventDefault();
                      if (listed.length === 0) return;
                      const step = e.key === 'ArrowDown' ? 1 : -1;
                      setCursor(c => (c + step + listed.length) % listed.length);
                      return;
                    }
                    if (e.key !== 'Enter') return;
                    const pick = listed[cursor] ?? listed[0];
                    if (pick) { setActiveTab(pick.id); setShowList(false); }
                  }}
                  placeholder="Filter open tabs…"
                />
              </div>
              {listed.length === 0 && <div className="menu-empty">No open tab matches</div>}
              <div id="tab-list-options" role="listbox" aria-label="Open tabs" className="stack">
              {listed.map((tab, i) => (
                <div
                  key={tab.id}
                  id={`tab-option-${tab.id}`}
                  role="option"
                  aria-selected={tab.id === activeTabId}
                  tabIndex={-1}
                  className={`menu-item tab-list-row${tab.id === activeTabId ? ' is-on' : ''}${i === cursor ? ' is-cursor' : ''}`}
                  onMouseEnter={() => setCursor(i)}
                  onClick={() => { setActiveTab(tab.id); setShowList(false); }}
                  onKeyDown={e => {
                    if (e.key === 'Enter' || e.key === ' ') {
                      e.preventDefault();
                      setActiveTab(tab.id);
                      setShowList(false);
                    }
                  }}
                  title={tabFileMissing(tab, onDisk)
                    ? `${tab.collectionPath} is not on disk any more`
                    : tab.collectionPath ?? 'Not saved to a file yet'}
                >
                  <span className={`grow${tabFileMissing(tab, onDisk) ? ' is-gone' : ''}`}>{nameOf(tab)}</span>
                  {tabDirty(tab) && <span className="tab-dirty" aria-label="Unsaved changes" />}
                  <span className="muted mono tab-list-path">
                    {tab.collectionPath ? shortPath(tab.collectionPath, 28) : 'unsaved'}
                  </span>
                  <button
                    className="btn is-ghost is-icon is-sm"
                    aria-label={`Close ${nameOf(tab)}`}
                    title={`Close ${nameOf(tab)}`}
                    onClick={e => { e.stopPropagation(); void requestClose(tab.id); }}
                  >
                    <X size={11} />
                  </button>
                </div>
              ))}
              </div>
              {tabs.length > 1 && (
                <>
                  <div className="menu-sep" />
                  <button
                    className="menu-item"
                    onClick={() => { setShowList(false); handleCloseAll(); }}
                    title="Close every open tab — a fresh one is left to type in"
                  >
                    <X size={11} /> Close all {tabs.length} tabs
                  </button>
                </>
              )}
            </div>
          </Popover>
        </div>
      )}

      <div className="new-tab">
        <button
          className="btn is-ghost is-icon"
          onClick={() => { if (!addTab()) toast.error('Every open tab has unsaved edits — close one first'); }}
          title="New request (⌘⇧T) · a grpcurl command comes in from the palette (⌘K)"
          aria-label="New request"
        >
          <Plus size={16} />
        </button>
      </div>

      <Seg
        className="layout-pick"
        label="Where the response sits"
        value={layout}
        onChange={next => setLayout(next)}
        options={[
          {
            value: 'columns' as const,
            label: <Columns2 size={12} />,
            disabled: !wideEnough,
            title: wideEnough
              ? 'Request and response side by side (⌘⌥L)'
              : 'This window is too narrow to hold both — the panes stack until it is wider',
          },
          { value: 'rows' as const, label: <Rows2 size={12} />, title: 'Response under the request (⌘⌥L)' },
        ]}
      />

      <div ref={moreRef} className="picker">
        <button
          className="btn is-ghost is-icon"
          onClick={() => setShowMore(v => !v)}
          title={`${activeLabel}: copy as grpcurl, share, save as, close tabs`}
          aria-haspopup="menu"
          aria-expanded={showMore}
          aria-label={`Actions for ${activeLabel}`}
        >
          <MoreHorizontal size={14} />
        </button>
        <Popover open={showMore} anchor={moreRef} align="end">
          <div className="menu">
            <div className="menu-group">{activeLabel}</div>
            <button
              className="menu-item"
              onClick={() => {
                setShowMore(false);
                void (isHttp ? handleCopyCurl() : handleCopyGrpcurl());
              }}
              disabled={!request.endpoint || isExecuting}
              title={
                !request.endpoint ? 'Pick an endpoint first'
                : isExecuting ? 'The call is still running'
                : `The same call, as a ${isHttp ? 'curl' : 'grpcurl'} command line`
              }
            >
              <Terminal size={13} /> Copy as {isHttp ? 'curl' : 'grpcurl'}
            </button>
            <button
              className="menu-item"
              onClick={() => { setShowMore(false); handleShare(); }}
              title={activeHasFile
                ? 'A link to this file in this workbench — it opens the file, not a copy of it'
                : 'This request is nowhere but here, so a share carries it — choose what goes in it'}
            >
              <Share2 size={13} /> {activeHasFile ? 'Copy a link to this file' : 'Share…'}
            </button>
            <button className="menu-item" onClick={() => { setShowMore(false); useStore.getState().requestSaveAs(); }}>
              <FileSymlink size={13} /> Save as…
            </button>
            <div className="menu-sep" />
            {tabs.length > 1 && (
              <button
                className="menu-item"
                onClick={() => { setShowMore(false); handleCloseOthers(activeTabId ?? ''); }}
              >
                <XCircle size={13} /> Close the other {tabs.length === 2 ? 'tab' : `${tabs.length - 1} tabs`}
              </button>
            )}
            {goneTabs.length > 0 && (
              <button
                className="menu-item"
                onClick={() => { setShowMore(false); handleCloseGone(); }}
                title={goneTabs.map(t => t.collectionPath).join('\n')}
              >
                <FileX size={13} /> Close {goneTabs.length} not on disk
              </button>
            )}
            <button className="menu-item" onClick={() => { setShowMore(false); handleCloseAll(); }}>
              <XCircle size={13} /> {tabs.length === 1 ? 'Close this tab' : `Close all ${tabs.length} tabs`}
            </button>
          </div>
        </Popover>
      </div>

      {ctxMenu && (() => {
        const target = tabs.find(t => t.id === ctxMenu.tabId);
        const index = tabs.findIndex(t => t.id === ctxMenu.tabId);
        const toTheRight = tabs.length - index - 1;
        return (
          <ContextMenu at={ctxMenu} onClose={closeMenu}>
            {target?.isPreview && (
              <>
                <button className="menu-item" onClick={() => { closeMenu(); pinTab(ctxMenu.tabId); }}>
                  <Pin size={13} /> Keep open
                </button>
                <div className="menu-sep" />
              </>
            )}
            <button
              className="menu-item"
              disabled={index <= 0}
              onClick={() => { closeMenu(); moveTab(index, index - 1); }}
            >
              <ChevronLeft size={13} /> Move left
            </button>
            <button
              className="menu-item"
              disabled={index < 0 || index >= tabs.length - 1}
              onClick={() => { closeMenu(); moveTab(index, index + 1); }}
            >
              <ChevronRight size={13} /> Move right
            </button>
            <div className="menu-sep" />
            <button className="menu-item" onClick={() => { closeMenu(); void requestClose(ctxMenu.tabId); }}>
              <X size={13} /> Close
            </button>
            <button
              className="menu-item"
              disabled={tabs.length < 2}
              onClick={() => handleCloseOthers(ctxMenu.tabId)}
            >
              <XCircle size={13} /> Close the other {tabs.length === 2 ? 'tab' : `${tabs.length - 1} tabs`}
            </button>
            <button
              className="menu-item"
              disabled={toTheRight === 0}
              onClick={() => handleCloseRight(ctxMenu.tabId)}
            >
              <ArrowRightToLine size={13} /> Close {toTheRight === 0 ? '' : `${toTheRight} `}to the right
            </button>
            <button
              className="menu-item"
              disabled={goneTabs.length === 0}
              onClick={() => { closeMenu(); handleCloseGone(); }}
              title={goneTabs.length === 0
                ? 'Every open tab still has a file'
                : goneTabs.map(t => t.collectionPath).join('\n')}
            >
              <FileX size={13} /> Close {goneTabs.length === 0 ? 'the ones' : goneTabs.length} not on disk
            </button>
            <button className="menu-item" onClick={handleCloseAll}>
              <XCircle size={13} /> {tabs.length === 1 ? 'Close this tab' : `Close all ${tabs.length} tabs`}
            </button>
            <div className="menu-sep" />
            <button className="menu-item" onClick={() => handleRename(ctxMenu.tabId)}>
              <Pencil size={13} /> Rename
            </button>
            <button className="menu-item" onClick={() => handleDuplicate(ctxMenu.tabId)}>
              <FileSymlink size={13} /> Duplicate
            </button>
            {target?.collectionPath && (
              <button
                className="menu-item"
                onClick={() => { closeMenu(); void copyToClipboard(target.collectionPath!); }}
              >
                <Copy size={13} /> Copy path
              </button>
            )}
          </ContextMenu>
        );
      })()}

      <ImportDialog open={showImport} onClose={() => setShowImport(false)} />

      <ShareDialog
        open={share !== null}
        headers={share?.headers ?? {}}
        bodies={request.bodies}
        onToggleHeader={toggleShareHeader}
        ttl={share?.ttl ?? 7}
        onTtl={setShareTtl}
        sharing={sharing}
        onCreate={handleCreateShare}
        onClose={closeShare}
        link={share?.link}
        expires={share?.expires}
      />
    </div>
  );
}

function ShareDialog({ open, headers, bodies, onToggleHeader, ttl, onTtl, sharing, onCreate, onClose, link, expires }: {
  open: boolean;
  headers: Record<string, boolean>;
  bodies: string[];
  onToggleHeader: (key: string) => void;
  ttl: number;
  onTtl: (v: number) => void;
  sharing: boolean;
  onCreate: () => void;
  link?: string;
  expires?: string;
  onClose: () => void;
}) {
  const sharesPath = useStore(s => s.sharesPath);
  const target = useStore(callAddress);
  const endpoint = useStore(s => s.request.endpoint);
  const headerValues = useStore(s => s.request.headers);
  const travelling = useMemo(
    () => [...new Set([endpoint, ...Object.values(headerValues), ...bodies].flatMap(findVariables))],
    [endpoint, headerValues, bodies],
  );
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (open && !el.open) el.showModal();
    if (!open && el.open) el.close();
  }, [open]);

  const keys = Object.keys(headers);

  return (
    <dialog
      ref={ref}
      className="modal"
      aria-label="Share request"
      onCancel={e => { e.preventDefault(); onClose(); }}
      onClose={() => onClose()}
      onClick={e => { if (e.target === ref.current) onClose(); }}
    >
      <div className="modal-head">
        <h2 className="modal-title">Share request</h2>
      </div>

      <div className="modal-body stack">
        <div className="note">
          A share writes this request to <span className="mono">{sharesPath}</span> on the machine
          running the workbench. Anyone who can reach this server and has the link can read it,
          until it expires.
          {hasToken() && ' This workbench asks for a token, so the reader needs that too — it is in the link it printed at startup.'}
        </div>

        <div>
          <div className="label">Headers</div>
          {keys.length === 0 && <div className="muted">No headers to share.</div>}
          {keys.map(key => {
            const secret = isSecretHeader(key);
            return (
              <label key={key} className="bar menu-check">
                <input type="checkbox" checked={headers[key]} onChange={() => onToggleHeader(key)} />
                <span className={headers[key] ? undefined : 'muted'}>{key}</span>
                {secret && (
                  <span className={headers[key] ? 'warn' : 'muted'}>
                    {headers[key]
                      ? 'this credential is written into the share as it is'
                      : 'carries a credential — left out by default'}
                  </span>
                )}
              </label>
            );
          })}
        </div>

        <div>
          <div className="label">Messages</div>
          <div className="muted">
            {count(bodies.length, 'message')}, {humanBytes(byteSize(bodies.join('')))}, sent as written
          </div>
          {bodyWarnings(bodies).map(w => (
            <div key={w.index} className="note is-warn">
              Message #{w.index + 1} {w.reason} — a share writes it to disk as it is.
            </div>
          ))}
        </div>

        {travelling.length > 0 && (
          <div>
            <div className="label">Variables</div>
            <div className="muted">
              <span className="mono">{travelling.map(v => `{{${v}}}`).join(' ')}</span>{' '}
              {travelling.length === 1 ? 'travels' : 'travel'} as written — the{' '}
              {travelling.length === 1 ? 'value stays' : 'values stay'} in this browser, and whoever
              opens the link answers for {travelling.length === 1 ? 'it' : 'them'}.
            </div>
          </div>
        )}

        <div>
          <div className="label">Target</div>
          <div className="muted">
            {target
              ? <>The link opens against <span className="mono">{target}</span> — where this call goes from here.</>
              : 'This request names no target, so the link opens against whatever the reader’s workbench points at.'}
          </div>
        </div>

        <div>
          <div className="label">Expires in</div>
          <Seg
            label="How long the link lasts"
            value={String(ttl)}
            onChange={d => onTtl(Number(d))}
            options={[1, 3, 7, 14, 30].map(d => ({ value: String(d), label: `${d}d` }))}
          />
        </div>
      </div>

      {link && (
        <div className="note share-link">
          <div className="label">The link{expires ? ` — expires ${expires}` : ''}</div>
          <input className="field mono" readOnly value={link} onFocus={e => e.currentTarget.select()} />
        </div>
      )}

      <div className="modal-foot">
        <button className="btn is-quiet" onClick={onClose}>{link ? 'Done' : 'Cancel'}</button>
        {link ? (
          <button className="btn is-primary" onClick={() => void copyToClipboard(link)} autoFocus>
            Copy again
          </button>
        ) : (
          <button className="btn is-primary" onClick={onCreate} disabled={sharing} autoFocus>
            {sharing ? 'Creating…' : 'Create link'}
          </button>
        )}
      </div>
    </dialog>
  );
}

function ImportDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (open && !el.open) el.showModal();
    if (!open && el.open) el.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      className="modal"
      aria-label="Import a command"
      onCancel={e => { e.preventDefault(); onClose(); }}
      onClose={() => onClose()}
      onClick={e => { if (e.target === ref.current) onClose(); }}
    >
      <div className="modal-head">
        <h2 className="modal-title">Import</h2>
        <button className="btn is-ghost is-icon" onClick={onClose} aria-label="Close">
          <X size={14} />
        </button>
      </div>
      <div className="modal-body">
        <ImportPanel onDone={onClose} />
      </div>
    </dialog>
  );
}

function stepTabs(
  e: React.KeyboardEvent<HTMLDivElement>,
  tabs: Tab[],
  index: number,
  act: {
    select: (id: string) => void;
    pin: (id: string) => void;
    close: (id: string) => void;
    menu: (id: string, el: HTMLElement) => void;
  },
) {
  const move = (next: number) => {
    e.preventDefault();
    const target = tabs[(next + tabs.length) % tabs.length];
    act.select(target.id);
    const strip = e.currentTarget.parentElement;
    const el = strip?.children[(next + tabs.length) % tabs.length];
    (el as HTMLElement | undefined)?.focus();
  };

  switch (e.key) {
    case 'ArrowRight': return move(index + 1);
    case 'ArrowLeft': return move(index - 1);
    case 'Home': return move(0);
    case 'End': return move(tabs.length - 1);
    case 'Enter':
    case ' ':
      e.preventDefault();
      act.select(tabs[index].id);
      if (tabs[index].isPreview) act.pin(tabs[index].id);
      return;
    case 'Delete':
    case 'Backspace':
      e.preventDefault();
      return act.close(tabs[index].id);
    case 'ContextMenu':
      e.preventDefault();
      return act.menu(tabs[index].id, e.currentTarget);
    case 'F10':
      if (!e.shiftKey) return;
      e.preventDefault();
      return act.menu(tabs[index].id, e.currentTarget);
    default:
  }
}
