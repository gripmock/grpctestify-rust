import { useEffect, useMemo, useRef, useState } from 'react';
import { Search, X } from 'lucide-react';
import { formatHotkey } from 'luvo/input/hotkeys';
import { LOCAL_KEYS } from '../../lib/hotkeys';
import { hotkeyCommands } from '../../lib/commands';
import { filterRows, groupRows, shortcutRows } from '../../lib/shortcut-rows';

interface Props {
  open: boolean;
  onClose: () => void;
}

export function KeyboardShortcutHelp({ open, onClose }: Props) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [search, setSearch] = useState('');
  const close = () => { setSearch(''); onClose(); };

  const all = useMemo(() => shortcutRows(), []);
  const rows = filterRows(all, search);
  const groups = groupRows(rows);
  const needle = search.trim().toLowerCase();
  const locals = LOCAL_KEYS.filter(k => needle === '' || `${k.where} ${k.keys}`.toLowerCase().includes(needle));
  const paletteKey = hotkeyCommands().find(c => c.id === 'view.palette')?.hotkey;

  useEffect(() => {
    const el = dialogRef.current;
    if (!el) return;
    if (open && !el.open) el.showModal();
    if (!open && el.open) el.close();
  }, [open]);

  return (
    <dialog
      ref={dialogRef}
      className="modal is-md"
      aria-label="Keyboard shortcuts"
      onCancel={e => { e.preventDefault(); close(); }}
      onClose={() => close()}
      onClick={e => { if (e.target === dialogRef.current) close(); }}
    >
      <div className="modal-head">
        <h2 className="modal-title">Keyboard shortcuts</h2>
        <div className="field-frame keys-search">
          <Search size={12} className="muted" />
          <input
            className="field"
            value={search}
            autoFocus
            placeholder="key or what it does…"
            onChange={e => setSearch(e.target.value)}
          />
          {search && (
            <button className="btn is-ghost is-icon is-sm" onClick={() => setSearch('')} aria-label="Clear filter">
              <X size={11} />
            </button>
          )}
        </div>
        {search && <span className="muted keys-count">{rows.length + locals.length} of {all.length + LOCAL_KEYS.length}</span>}
        <button className="btn is-ghost is-icon" onClick={close} aria-label="Close">
          <X size={14} />
        </button>
      </div>

      <div className="modal-body keys-body">
        <dl className="keys-list">
          {groups.map(g => (
            <div key={g.group} className="keys-group" role="presentation">
              <div className="keys-group-name">{g.group}</div>
              {g.rows.map(r => (
                <div key={`${g.group}-${r.keys}-${r.what}`} className="keys-row">
                  <dt><kbd className="kbd">{r.keys}</kbd></dt>
                  <dd>{r.what}</dd>
                </div>
              ))}
            </div>
          ))}

        </dl>

        {locals.length > 0 && (
          <dl className="keys-list keys-local">
            <div className="keys-group" role="presentation">
              <div className="keys-group-name">Inside a control</div>
              {locals.map(k => (
                <div key={k.where} className="keys-row">
                  <dt className="keys-where">{k.where}</dt>
                  <dd className="mono">{k.keys}</dd>
                </div>
              ))}
            </div>
          </dl>
        )}

        {rows.length === 0 && locals.length === 0 && (
          <div className="empty-state">Nothing matches “{search}”.</div>
        )}
      </div>

      <div className="modal-foot keys-foot">
        <kbd className="kbd">?</kbd> opens this panel
        <span className="muted"> · </span>
        <kbd className="kbd">{paletteKey ? formatHotkey(paletteKey) : '?'}</kbd> runs any command by name
      </div>
    </dialog>
  );
}
