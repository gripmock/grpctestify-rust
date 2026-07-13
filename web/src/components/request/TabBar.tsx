import { useState, useEffect, useRef, useCallback } from 'react';
import { useStore } from '../../lib/store';
import { Plus, X, XCircle, FileSymlink, Pencil, Trash2, Share2, ChevronLeft, ChevronRight } from 'lucide-react';
import { useModal } from '../ui/ModalContext';
import { useToast } from '../ui/ToastContext';

interface CtxMenu {
  x: number;
  y: number;
  tabId: string;
}

const ctxItemStyle: React.CSSProperties = {
  display: 'flex', alignItems: 'center', gap: 6, padding: '6px 12px', fontSize: 12,
  cursor: 'pointer', transition: 'background 0.1s',
};

export function TabBar() {
  const tabs = useStore(s => s.tabs);
  const activeTabId = useStore(s => s.activeTabId);
  const setActiveTab = useStore(s => s.setActiveTab);
  const removeTab = useStore(s => s.removeTab);
  const addTab = useStore(s => s.addTab);
  const setTabLabel = useStore(s => s.setTabLabel);
  const modal = useModal();
  const toast = useToast();

  const scrollRef = useRef<HTMLDivElement>(null);
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
    el.addEventListener('scroll', updateScrollState);
    const ro = new ResizeObserver(updateScrollState);
    ro.observe(el);
    return () => { el.removeEventListener('scroll', updateScrollState); ro.disconnect(); };
  }, [tabs.length, updateScrollState]);

  const scrollBy = useCallback((dir: number) => {
    scrollRef.current?.scrollBy({ left: dir * 200, behavior: 'smooth' });
  }, []);

  const [ctxMenu, setCtxMenu] = useState<CtxMenu | null>(null);
  const [showShareDialog, setShowShareDialog] = useState(false);
  const [shareHeaders, setShareHeaders] = useState<Record<string, boolean>>({});
  const [shareTtl, setShareTtl] = useState(7);
  const [sharing, setSharing] = useState(false);
  const ctxRef = useRef<HTMLDivElement>(null);

  const closeMenu = useCallback(() => setCtxMenu(null), []);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ctxRef.current && !ctxRef.current.contains(e.target as Node)) {
        closeMenu();
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [closeMenu]);

  const handleContextMenu = useCallback((e: React.MouseEvent, tabId: string) => {
    e.preventDefault();
    setCtxMenu({ x: e.clientX, y: e.clientY, tabId });
  }, []);

  const handleRename = useCallback(async (tabId: string) => {
    const tab = tabs.find(t => t.id === tabId);
    if (!tab) return;
    const name = await modal.prompt('Rename tab', 'Tab name:', tab.label);
    if (name) setTabLabel(tabId, name);
    closeMenu();
  }, [tabs, modal, setTabLabel, closeMenu]);

  const handleDuplicate = useCallback((tabId: string) => {
    const tab = tabs.find(t => t.id === tabId);
    if (!tab) return;
    addTab({ endpoint: tab.endpoint, headers: tab.headers, bodies: tab.bodies });
    closeMenu();
  }, [tabs, addTab, closeMenu]);

  const handleCloseOthers = useCallback((tabId: string) => {
    for (const t of tabs) {
      if (t.id !== tabId) removeTab(t.id);
    }
    closeMenu();
  }, [tabs, removeTab, closeMenu]);

  const handleCloseRight = useCallback((tabId: string) => {
    const idx = tabs.findIndex(t => t.id === tabId);
    if (idx === -1) return;
    for (let i = idx + 1; i < tabs.length; i++) {
      removeTab(tabs[i].id);
    }
    closeMenu();
  }, [tabs, removeTab, closeMenu]);

  const SENSITIVE_HEADERS = ['authorization', 'cookie', 'x-api-key'];

  const handleShare = useCallback(async () => {
    const tab = tabs.find(t => t.id === activeTabId);
    if (!tab) return;

    // Collection tab → share as /c/path
    if (tab.collectionPath) {
      const url = `${window.location.origin}/c/${tab.collectionPath}`;
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(url);
      } else {
        const ta = document.createElement('textarea');
        ta.value = url; ta.style.position = 'fixed'; ta.style.opacity = '0';
        document.body.appendChild(ta); ta.select();
        document.execCommand('copy');
        document.body.removeChild(ta);
      }
      toast.success('Collection link copied!');
      return;
    }

    // Ad-hoc tab → show share dialog
    const initialHeaders: Record<string, boolean> = {};
    for (const key of Object.keys(tab.headers)) {
      initialHeaders[key] = !SENSITIVE_HEADERS.includes(key.toLowerCase());
    }
    setShareHeaders(initialHeaders);
    setShareTtl(7);
    setShowShareDialog(true);
  }, [tabs, activeTabId, toast]);

  const handleCreateShare = useCallback(async () => {
    const tab = tabs.find(t => t.id === activeTabId);
    if (!tab) return;
    setSharing(true);

    const filteredHeaders: Record<string, string> = {};
    for (const [key, val] of Object.entries(tab.headers)) {
      if (shareHeaders[key] !== false) {
        filteredHeaders[key] = val;
      }
    }

    try {
      const res = await fetch('/api/share', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          endpoint: tab.endpoint,
          headers: Object.keys(filteredHeaders).length > 0 ? filteredHeaders : undefined,
          bodies: tab.bodies,
          ttl_days: shareTtl,
        }),
      });
      if (!res.ok) { toast.error('Failed to create share'); return; }
      const data = await res.json();
      const url = `${window.location.origin}${data.url}`;
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(url);
      } else {
        const ta = document.createElement('textarea');
        ta.value = url; ta.style.position = 'fixed'; ta.style.opacity = '0';
        document.body.appendChild(ta); ta.select();
        document.execCommand('copy');
        document.body.removeChild(ta);
      }
      const expires = new Date(data.expires_at).toLocaleDateString();
      toast.success(`Link copied! Expires ${expires}`);
      setShowShareDialog(false);
    } catch {
      toast.error('Failed to create share');
    } finally {
      setSharing(false);
    }
  }, [tabs, activeTabId, shareHeaders, shareTtl, toast]);

  if (!tabs || tabs.length === 0) return null;

  return (
    <div style={{
      display: 'flex', alignItems: 'center', gap: 0, marginBottom: 8,
      borderBottom: '1px solid var(--border)', minHeight: 32, overflow: 'hidden',
      position: 'relative',
    }}>
      <div style={{ position: 'relative', flex: 1, minWidth: 0 }}>
        <div ref={scrollRef} className="tabs-scroll" style={{
          display: 'flex', alignItems: 'stretch', gap: 0, overflowX: 'auto',
          scrollbarWidth: 'none',
        }}>
          {tabs.map(tab => {
            const isActive = tab.id === activeTabId;
            return (
              <div key={tab.id} style={{
                display: 'flex', alignItems: 'center', gap: 4,
                padding: '5px 8px 5px 12px', fontSize: 12, cursor: 'pointer',
                whiteSpace: 'nowrap', flexShrink: 0,
                borderBottom: isActive ? '2px solid var(--accent)' : '2px solid transparent',
                color: isActive ? 'var(--accent)' : 'var(--text-secondary)',
                fontWeight: isActive ? 600 : 400,
                background: isActive ? 'var(--bg-primary)' : 'transparent',
                transition: 'all 0.1s ease',
                minWidth: 80, maxWidth: 180,
              }}
                onClick={() => setActiveTab(tab.id)}
                onContextMenu={e => handleContextMenu(e, tab.id)}
                onMouseEnter={e => { if (!isActive) e.currentTarget.style.background = 'var(--bg-tertiary)'; }}
                onMouseLeave={e => { if (!isActive) e.currentTarget.style.background = 'transparent'; }}
              >
                <span style={{
                  overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1,
                }}>
                  {tab.label}
                </span>
                <button onClick={e => { e.stopPropagation(); removeTab(tab.id); }} style={{
                  display: 'flex', alignItems: 'center', justifyContent: 'center',
                  width: 18, height: 18, borderRadius: 3, border: 'none', background: 'none',
                  cursor: 'pointer', color: isActive ? 'var(--accent)' : 'var(--text-muted)',
                  opacity: 0, flexShrink: 0,
                  transition: 'all 0.1s ease',
                }}
                  className="tab-close-btn"
                  onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-tertiary)'; e.currentTarget.style.opacity = '1'; }}
                  onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.opacity = '0'; }}
                >
                  <X size={12} />
                </button>
              </div>
            );
          })}

          <style>{`
            .tab-close-btn { opacity: 0; }
            div:has(> .tab-close-btn):hover .tab-close-btn { opacity: 1 !important; }
            .tabs-scroll { -ms-overflow-style: none; }
            .tabs-scroll::-webkit-scrollbar { display: none; }
          `}</style>
        </div>

        {canScrollLeft && (
          <button onClick={() => scrollBy(-1)} style={{
            position: 'absolute', left: 0, top: 0, bottom: 0, width: 24,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            border: 'none', background: 'linear-gradient(to right, var(--bg-primary), transparent)',
            cursor: 'pointer', color: 'var(--text-muted)', padding: 0,
          }}>
            <ChevronLeft size={14} />
          </button>
        )}
        {canScrollRight && (
          <button onClick={() => scrollBy(1)} style={{
            position: 'absolute', right: 0, top: 0, bottom: 0, width: 24,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            border: 'none', background: 'linear-gradient(to left, var(--bg-primary), transparent)',
            cursor: 'pointer', color: 'var(--text-muted)', padding: 0,
          }}>
            <ChevronRight size={14} />
          </button>
        )}
      </div>

      <button onClick={handleShare} style={{
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        width: 28, height: 28, borderRadius: 4, border: 'none', background: 'none',
        cursor: 'pointer', color: 'var(--text-muted)', flexShrink: 0,
        transition: 'all 0.1s ease',
      }}
        onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-tertiary)'; e.currentTarget.style.color = 'var(--text-primary)'; }}
        onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--text-muted)'; }}
        title="Share"
      >
        <Share2 size={14} />
      </button>

      <button onClick={() => addTab()} style={{
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        width: 28, height: 28, borderRadius: 4, border: 'none', background: 'none',
        cursor: 'pointer', color: 'var(--text-muted)', flexShrink: 0,
        transition: 'all 0.1s ease', marginRight: 2,
      }}
        onMouseEnter={e => { e.currentTarget.style.background = 'var(--bg-tertiary)'; e.currentTarget.style.color = 'var(--text-primary)'; }}
        onMouseLeave={e => { e.currentTarget.style.background = 'transparent'; e.currentTarget.style.color = 'var(--text-muted)'; }}
        title="New tab"
      >
        <Plus size={16} />
      </button>

      {ctxMenu && (
        <div ref={ctxRef} style={{
          position: 'fixed', left: ctxMenu.x, top: ctxMenu.y, zIndex: 1000,
          background: 'var(--bg-primary)', border: '1px solid var(--border)',
          borderRadius: 6, boxShadow: '0 4px 16px rgba(0,0,0,0.2)',
          padding: '4px 0', minWidth: 150,
        }}>
          <div onClick={() => { closeMenu(); removeTab(ctxMenu.tabId); }} style={ctxItemStyle}>
            <X size={13} /> Close
          </div>
          <div onClick={() => handleCloseOthers(ctxMenu.tabId)} style={ctxItemStyle}>
            <XCircle size={13} /> Close others
          </div>
          <div onClick={() => handleCloseRight(ctxMenu.tabId)} style={ctxItemStyle}>
            <Trash2 size={13} /> Close right
          </div>
          <div style={{ height: 1, background: 'var(--border)', margin: '4px 0' }} />
          <div onClick={() => handleRename(ctxMenu.tabId)} style={ctxItemStyle}>
            <Pencil size={13} /> Rename
          </div>
          <div onClick={() => handleDuplicate(ctxMenu.tabId)} style={ctxItemStyle}>
            <FileSymlink size={13} /> Duplicate
          </div>
        </div>
      )}

      {showShareDialog && (
        <div onClick={() => setShowShareDialog(false)} style={{
          position: 'fixed', inset: 0, zIndex: 10000,
          display: 'flex', alignItems: 'center', justifyContent: 'center',
          background: 'rgba(0,0,0,0.5)',
        }}>
          <div onClick={e => e.stopPropagation()} style={{
            background: 'var(--bg-primary)', borderRadius: 10,
            border: '1px solid var(--border)', boxShadow: '0 8px 32px rgba(0,0,0,0.3)',
            width: 400, maxWidth: '90vw', padding: 0,
          }}>
            <div style={{ padding: '16px 20px', borderBottom: '1px solid var(--border)' }}>
              <div style={{ fontSize: 14, fontWeight: 600 }}>Share Request</div>
              <div style={{ fontSize: 11, color: 'var(--text-muted)', marginTop: 4 }}>
                All fields below are optional. Uncheck headers you want to exclude.
              </div>
            </div>

            <div style={{ padding: '12px 20px', maxHeight: 300, overflowY: 'auto' }}>
              {Object.keys(shareHeaders).length > 0 && (
                <div style={{ marginBottom: 12 }}>
                  <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-secondary)', marginBottom: 6, textTransform: 'uppercase', letterSpacing: '0.5px' }}>
                    Headers
                  </div>
                  {Object.keys(shareHeaders).map(key => (
                    <label key={key} style={{
                      display: 'flex', alignItems: 'center', gap: 6, padding: '3px 0',
                      fontSize: 12, cursor: 'pointer',
                      color: shareHeaders[key] ? 'var(--text-primary)' : 'var(--text-muted)',
                    }}>
                      <input type="checkbox" checked={shareHeaders[key]}
                        onChange={() => setShareHeaders(p => ({ ...p, [key]: !p[key] }))}
                        style={{ accentColor: 'var(--accent)' }} />
                      {key}
                    </label>
                  ))}
                </div>
              )}
              {Object.keys(shareHeaders).length === 0 && (
                <div style={{ fontSize: 12, color: 'var(--text-muted)', marginBottom: 12 }}>
                  No headers to share.
                </div>
              )}

              <div>
                <div style={{ fontSize: 11, fontWeight: 600, color: 'var(--text-secondary)', marginBottom: 6, textTransform: 'uppercase', letterSpacing: '0.5px' }}>
                  Expires in
                </div>
                <select value={shareTtl} onChange={e => setShareTtl(Number(e.target.value))} style={{
                  padding: '6px 10px', fontSize: 12, borderRadius: 6,
                  border: '1px solid var(--border)', background: 'var(--bg-primary)',
                  color: 'var(--text-primary)', outline: 'none', width: '100%',
                }}>
                  <option value={1}>1 day</option>
                  <option value={3}>3 days</option>
                  <option value={7}>7 days</option>
                  <option value={14}>14 days</option>
                  <option value={30}>30 days</option>
                </select>
              </div>
            </div>

            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 6, padding: '12px 20px', borderTop: '1px solid var(--border)' }}>
              <button onClick={() => setShowShareDialog(false)} style={{
                padding: '6px 14px', fontSize: 12, borderRadius: 6, cursor: 'pointer',
                border: '1px solid var(--border)', background: 'var(--bg-primary)',
                color: 'var(--text-primary)',
              }}>
                Cancel
              </button>
              <button onClick={handleCreateShare} disabled={sharing} style={{
                padding: '6px 14px', fontSize: 12, borderRadius: 6, cursor: sharing ? 'wait' : 'pointer',
                border: 'none', background: 'var(--accent)', color: '#fff',
                opacity: sharing ? 0.7 : 1,
              }}>
                {sharing ? 'Creating…' : 'Create link'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
