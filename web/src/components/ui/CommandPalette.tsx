import { useEffect, useMemo, useRef, useState } from 'react';
import { useStore } from '../../lib/store';
import { COMMANDS, filterCommands } from '../../lib/commands';
import type { CommandUi } from '../../lib/commands';
import { formatHotkey } from 'luvo/input/hotkeys';
import { rankPaths } from 'luvo/data/fuzzy';
import { familyOf } from '../../lib/tree';
import { FileJson, Globe, Network } from 'lucide-react';
import { useToast } from 'luvo/ui/ToastContext';
import { plural } from 'luvo/data/plural';

const FILES_SHOWN = 8;
const PAGE = 8;

export function CommandPalette({ open, onClose, ui }: { open: boolean; onClose: () => void; ui: CommandUi }) {
  const toast = useToast();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [query, setQuery] = useState('');
  const [cursor, setCursor] = useState(0);

  useEffect(() => {
    const el = dialogRef.current;
    if (!el) return;
    if (open && !el.open) { el.showModal(); setQuery(''); setCursor(0); }
    if (!open && el.open) el.close();
  }, [open]);

  const matches = useMemo(
    () => (open ? filterCommands(COMMANDS, query, useStore.getState()) : []),
    [open, query],
  );

  const found = useMemo(() => {
    if (!open) return [];
    const all = useStore.getState().collections.filter(c => !c.is_dir && familyOf(c.path) !== 'unknown');
    return rankPaths(all.map(c => c.path), query);
  }, [open, query]);

  const files = found.slice(0, FILES_SHOWN);
  const hidden = found.length - files.length;

  const total = matches.length + files.length;

  const listRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const el = listRef.current?.querySelector<HTMLElement>('.menu-item.is-on');
    el?.scrollIntoView({ block: 'nearest' });
  }, [cursor, open]);

  const runAt = (i: number) => {
    if (i < matches.length) {
      const command = matches[i];
      if (!command) return;
      onClose();
      command.run(useStore.getState(), ui);
      return;
    }
    const file = files[i - matches.length];
    if (!file) return;
    onClose();
    void useStore.getState().loadCollection(file).then(opened => {
      if (!opened) toast.error(`${file} could not be opened — the list may be out of date`);
    });
  };

  return (
    <dialog
      ref={dialogRef}
      className="modal palette"
      aria-label="Command palette"
      onCancel={e => { e.preventDefault(); onClose(); }}
      onClose={() => onClose()}
      onClick={e => { if (e.target === dialogRef.current) onClose(); }}
    >
      <div className="field-frame">
        <input
          className="field"
          autoFocus
          value={query}
          placeholder="Run, save, open a panel…"
          onChange={e => { setQuery(e.target.value); setCursor(0); }}
          onKeyDown={e => {
            const last = Math.max(0, total - 1);
            const to = (n: number) => { e.preventDefault(); setCursor(Math.min(last, Math.max(0, n))); };
            if (e.key === 'ArrowDown') to(cursor + 1);
            if (e.key === 'ArrowUp') to(cursor - 1);
            if (e.key === 'PageDown') to(cursor + PAGE);
            if (e.key === 'PageUp') to(cursor - PAGE);
            if (e.key === 'Home') to(0);
            if (e.key === 'End') to(last);
            if (e.key === 'Enter') { e.preventDefault(); runAt(cursor); }
          }}
        />
      </div>

      <div ref={listRef} className="menu palette-list">
        {total === 0 && (
          <div className="empty">Nothing matches — the palette searches commands and the project's files.</div>
        )}
        {matches.length > 0 && <div className="menu-group">actions</div>}
        {matches.map((c, i) => (
          <button
            key={c.id}
            className={`menu-item${i === cursor ? ' is-on' : ''}`}
            onMouseEnter={() => setCursor(i)}
            onClick={() => runAt(i)}
          >
            <span className="grow">{c.title}</span>
            <span className="muted">{c.category}</span>
            {c.hotkey && <kbd className="kbd">{formatHotkey(c.hotkey)}</kbd>}
          </button>
        ))}

        {files.length > 0 && <div className="menu-group">go to</div>}
        {files.map((path, i) => {
          const index = matches.length + i;
          const family = familyOf(path);
          return (
            <button
              key={path}
              className={`menu-item${index === cursor ? ' is-on' : ''}`}
              onMouseEnter={() => setCursor(index)}
              onClick={() => runAt(index)}
            >
              {family === 'httf'
                ? <Globe size={12} className="is-httf" />
                : family === 'apif'
                  ? <Network size={12} className="is-apif" />
                  : <FileJson size={12} className="is-gctf" />}
              <span className="mono grow">{path}</span>
            </button>
          );
        })}
        {hidden > 0 && (
          <div className="menu-group">{hidden} more {plural(hidden, 'file')} match — keep typing</div>
        )}
      </div>
    </dialog>
  );
}
