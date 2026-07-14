import { useMemo, useState, useCallback, useRef, useEffect } from 'react';
import { useStore } from '../../lib/store';
import { useModal } from '../ui/ModalContext';
import { useToast } from '../ui/ToastContext';
import type { TreeNode } from '../../lib/types';
import { btn, colors } from '../../lib/theme';
import { FileJson, Folder, FolderOpen, ChevronRight, RefreshCw, Search, Tag, Pencil, Trash2, FolderPlus, Copy } from 'lucide-react';

import { buildTree, sortTree, filterTree, collectTags } from '../../lib/tree';

interface CtxMenu {
  x: number;
  y: number;
  node: TreeNode;
}

export function Sidebar() {
  const collections = useStore(s => s.collections);
  const loadCollection = useStore(s => s.loadCollection);
  const selected = useStore(s => s.selectedCollection);
  const refreshCollections = useStore(s => s.refreshCollections);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [search, setSearch] = useState('');
  const [activeTags, setActiveTags] = useState<Set<string>>(new Set());
  const [expandedTags, setExpandedTags] = useState(false);
  const [ctxMenu, setCtxMenu] = useState<CtxMenu | null>(null);
  const ctxRef = useRef<HTMLDivElement>(null);
  const modal = useModal();
  const toast = useToast();

  const allTags = useMemo(() => collectTags(collections), [collections]);

  const tree = useMemo(() => {
    const sorted = sortTree(buildTree(collections));
    return (search || activeTags.size > 0) ? filterTree(sorted, search, activeTags) : sorted;
  }, [collections, search, activeTags]);

  useMemo(() => {
    const init = new Set(expanded);
    for (const n of tree) { if (n.isDir) init.add(n.path); }
    if (init.size !== expanded.size) setExpanded(init);
  }, [tree.length === 0 ? 0 : 1]);

  const toggle = (path: string) => {
    setExpanded(prev => { const next = new Set(prev); if (next.has(path)) next.delete(path); else next.add(path); return next; });
  };

  const toggleTag = (tag: string) => {
    setActiveTags(prev => {
      const next = new Set(prev);
      if (next.has(tag)) next.delete(tag); else next.add(tag);
      return next;
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

  
  const handleNewFolder = async (parentPath: string) => {
    const name = await modal.prompt('New Folder', 'Folder name:');
    if (!name?.trim()) return;
    const fullPath = parentPath ? `${parentPath}/${name.trim()}` : name.trim();
    try {
      const res = await fetch(`/api/dir/${fullPath}`, { method: 'POST' });
      if (!res.ok) { toast.error('Failed to create folder'); return; }
      refreshCollections();
    } catch { toast.error('Failed to create folder'); }
  };

  const handleNewFile = async (parentPath: string) => {
    const name = await modal.prompt('New File', 'File name (without .gctf):');
    if (!name?.trim()) return;
    const fileName = `${name.trim()}.gctf`;
    const fullPath = parentPath ? `${parentPath}/${fileName}` : fileName;
    try {
      const res = await fetch('/api/save', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ path: fullPath, content: '--- ENDPOINT ---\n\n--- REQUEST ---\n{}\n' }),
      });
      if (!res.ok) { toast.error('Failed to create file'); return; }
      refreshCollections();
    } catch { toast.error('Failed to create file'); }
  };
  
  const handleMove = async (fromPath: string, toPath?: string) => {
    const to = toPath || await modal.prompt('Move / Rename', 'Move to path (rename if same parent):', fromPath);
    if (!to?.trim()) return;
    try {
      const res = await fetch('/api/move', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ from: fromPath, to: to.trim() }),
      });
      if (!res.ok) { const err = await res.text().catch(() => ''); toast.error(err || 'Failed to move'); return; }
      refreshCollections();
    } catch { toast.error('Failed to move'); }
  };

  
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
    <div style={{ padding: 8 }}>
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 6 }}>
        <span style={{ fontSize: 10, fontWeight: 600, color: 'var(--text-muted)', textTransform: 'uppercase', letterSpacing: '0.6px' }}>Collections</span>
        <button onClick={refreshCollections} style={btn('ghost', 'sm')} title="Refresh"><RefreshCw size={12} /></button>
      </div>

      {}
      <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 6, border: '1px solid var(--border)', borderRadius: 5, padding: '3px 6px', background: 'var(--bg-primary)' }}>
        <Search size={12} style={{ color: 'var(--text-muted)', flexShrink: 0 }} />
        <input value={search} onChange={e => setSearch(e.target.value)} placeholder="Filter…"
          style={{ flex: 1, border: 'none', background: 'transparent', fontSize: 11, color: 'var(--text-primary)', outline: 'none' }} />
        {search && <button onClick={() => setSearch('')} style={{ ...btn('ghost', 'sm'), fontSize: 10, padding: '0 3px' }}>✕</button>}
      </div>

      {}
      {allTags.length > 0 && (
        <div style={{ marginBottom: 6 }}>
          <div style={{ display: 'flex', flexWrap: 'wrap', gap: 3 }}>
            {(() => {
              const MAX_VISIBLE = 5;
              const showAll = expandedTags;
              const visible = showAll ? allTags : allTags.slice(0, MAX_VISIBLE);
              return visible.map(({ tag, count }) => (
                <button key={tag} onClick={() => toggleTag(tag)} style={{
                  display: 'flex', alignItems: 'center', gap: 2, padding: '2px 5px', borderRadius: 4,
                  fontSize: 10, fontWeight: 500, cursor: 'pointer', border: '1px solid var(--border)',
                  background: activeTags.has(tag) ? colors.accent : 'var(--bg-primary)',
                  color: activeTags.has(tag) ? '#fff' : 'var(--text-secondary)',
                  transition: 'all 0.1s ease',
                }}>
                  <Tag size={9} />
                  {tag}
                  <span style={{
                    fontSize: 9, opacity: 0.7, marginLeft: 1,
                    color: activeTags.has(tag) ? 'rgba(255,255,255,0.8)' : 'var(--text-muted)',
                  }}>
                    {count}
                  </span>
                </button>
              ));
            })()}
          </div>
          <div style={{ display: 'flex', gap: 4, marginTop: 3 }}>
            {allTags.length > 5 && (
              <button onClick={() => setExpandedTags(v => !v)} style={{ ...btn('ghost', 'sm'), fontSize: 9, padding: '1px 4px', color: 'var(--text-muted)' }}>
                {expandedTags ? '− less' : `+${allTags.length - 5} more`}
              </button>
            )}
            {activeTags.size > 0 && (
              <button onClick={() => setActiveTags(new Set())} style={{ ...btn('ghost', 'sm'), fontSize: 9, padding: '1px 4px', color: 'var(--error)' }}>
                ✕ clear filter
              </button>
            )}
          </div>
        </div>
      )}

      {tree.length === 0 && (
        <div style={{ padding: '12px 0', fontSize: 12, color: 'var(--text-muted)', textAlign: 'center' }}>
          {search || activeTags.size > 0 ? 'No matches' : 'No .gctf files found.'}
        </div>
      )}

      <TreeNodes nodes={tree} depth={0} expanded={expanded} onToggle={toggle} selected={selected} onSelect={loadCollection}
        onContextMenu={handleContextMenu} onNewFolder={handleNewFolder} onNewFile={handleNewFile} onMove={handleMove}
        onDragStart={handleDragStart} onDragOver={handleDragOver} onDragLeave={handleDragLeave} onDrop={handleDrop}
        dragOverPath={dragOverPath} />

      {}
      {ctxMenu && (
        <div ref={ctxRef} style={{
          position: 'fixed', left: ctxMenu.x, top: ctxMenu.y, zIndex: 1000,
          background: 'var(--bg-primary)', border: '1px solid var(--border)',
          borderRadius: 6, boxShadow: '0 4px 16px rgba(0,0,0,0.2)',
          padding: '4px 0', minWidth: 150,
        }}>
          {ctxMenu.node.isDir && (
            <>
              <div onClick={() => { handleNewFolder(ctxMenu.node.path); setCtxMenu(null); }} style={ctxItemStyle}>
                <FolderPlus size={13} /> New folder
              </div>
              <div onClick={() => { handleNewFile(ctxMenu.node.path); setCtxMenu(null); }} style={ctxItemStyle}>
                <FileJson size={13} /> New file
              </div>
            </>
          )}
          <div onClick={() => { handleMove(ctxMenu.node.path); setCtxMenu(null); }} style={ctxItemStyle}>
            <Pencil size={13} /> Rename / Move
          </div>
          {!ctxMenu.node.isDir && (<>
            <div onClick={() => {
              const p = ctxMenu.node.path;
              if (navigator.clipboard?.writeText) {
                navigator.clipboard.writeText(p);
              } else {
                const ta = document.createElement('textarea');
                ta.value = p; ta.style.position = 'fixed'; ta.style.opacity = '0';
                document.body.appendChild(ta); ta.select();
                document.execCommand('copy');
                document.body.removeChild(ta);
              }
              setCtxMenu(null);
            }} style={ctxItemStyle}>
              <Copy size={13} /> Copy path
            </div>
            <div onClick={async () => {
              const ok = await modal.confirm('Delete', `Delete "${ctxMenu.node.path}"?`, { confirmText: 'Delete', cancelText: 'Cancel' });
              if (ok) {
                fetch(`/api/collections/${ctxMenu.node.path}`, { method: 'DELETE' })
                  .then(r => { if (r.ok) refreshCollections(); })
                  .catch(() => {});
              }
              setCtxMenu(null);
            }} style={{ ...ctxItemStyle, color: 'var(--error)' }}>
              <Trash2 size={13} /> Delete
            </div>
            </>)}
        </div>
      )}
    </div>
  );
}

const ctxItemStyle: React.CSSProperties = {
  display: 'flex', alignItems: 'center', gap: 6, padding: '6px 12px', fontSize: 12,
  cursor: 'pointer', transition: 'background 0.1s',
};

function TreeNodes({ nodes, depth, expanded, onToggle, selected, onSelect, onContextMenu, onNewFolder, onNewFile, onMove, onDragStart, onDragOver, onDragLeave, onDrop, dragOverPath }: {
  nodes: TreeNode[]; depth: number; expanded: Set<string>; selected: string | null;
  onToggle: (p: string) => void; onSelect: (p: string) => void;
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
    <TreeNodeRow key={node.path} node={node} depth={depth} expanded={expanded}
      onToggle={onToggle} selected={selected} onSelect={onSelect}
      onContextMenu={onContextMenu} onNewFolder={onNewFolder} onNewFile={onNewFile} onMove={onMove}
      onDragStart={onDragStart} onDragOver={onDragOver} onDragLeave={onDragLeave} onDrop={onDrop}
      dragOverPath={dragOverPath} />
  ))}</>;
}

function TreeNodeRow({ node, depth, expanded, onToggle, selected, onSelect, onContextMenu, onNewFolder, onNewFile, onMove, onDragStart, onDragOver, onDragLeave, onDrop, dragOverPath }: {
  node: TreeNode; depth: number; expanded: Set<string>; selected: string | null;
  onToggle: (p: string) => void; onSelect: (p: string) => void;
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
  const hasTags = !node.isDir && node.tags && node.tags.length > 0;
  const isDragOver = !node.isDir ? false : dragOverPath === node.path;

  return (
    <>
      <div onClick={() => { if (node.isDir) onToggle(node.path); else onSelect(node.path); }}
        onContextMenu={e => onContextMenu(e, node)}
        draggable={!node.isDir}
        onDragStart={e => onDragStart?.(e, node)}
        onDragOver={e => onDragOver?.(e, node)}
        onDragLeave={e => onDragLeave?.(e)}
        onDrop={e => onDrop?.(e, node)}
        style={{
          display: 'flex', alignItems: 'center', gap: 3, padding: '3px 6px', paddingLeft: 6 + depth * 14,
          borderRadius: 4, cursor: 'pointer', fontSize: 12,
          background: isDragOver ? `${colors.accent}25` : isSelected ? colors.accent : 'transparent',
          color: isSelected ? '#fff' : 'var(--text-primary)',
          outline: isDragOver ? `2px dashed ${colors.accent}` : 'none',
          outlineOffset: -2,
          transition: 'background 0.1s ease',
          userSelect: 'none',
        }}
        onMouseEnter={e => { if (!isSelected && !isDragOver) (e.currentTarget as HTMLElement).style.background = 'var(--bg-tertiary)'; }}
        onMouseLeave={e => { if (!isSelected && !isDragOver) (e.currentTarget as HTMLElement).style.background = 'transparent'; }}
      >
        {node.isDir ? (isExpanded ? <ChevronRight size={10} style={{ transform: 'rotate(90deg)' }} /> : <ChevronRight size={10} />) : <span style={{ width: 10 }} />}
        {node.isDir ? (isExpanded ? <FolderOpen size={13} /> : <Folder size={13} />) : <FileJson size={13} />}
        <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap', flex: 1 }}>{node.name}</span>
        {hasTags && node.tags!.slice(0, 2).map(t => (
          <span key={t} style={{
            fontSize: 9, padding: '0 4px', borderRadius: 3,
            background: isSelected ? 'rgba(255,255,255,0.25)' : 'var(--bg-tertiary)',
            color: isSelected ? 'rgba(255,255,255,0.9)' : 'var(--text-muted)',
            whiteSpace: 'nowrap',
          }}>
            {t}
          </span>
        ))}
      </div>
      {node.isDir && isExpanded && <TreeNodes nodes={node.children} depth={depth + 1} expanded={expanded} onToggle={onToggle} selected={selected} onSelect={onSelect} onContextMenu={onContextMenu} onNewFolder={onNewFolder} onNewFile={onNewFile} onMove={onMove} onDragStart={onDragStart} onDragOver={onDragOver} onDragLeave={onDragLeave} onDrop={onDrop} dragOverPath={dragOverPath} />}
    </>
  );
}
