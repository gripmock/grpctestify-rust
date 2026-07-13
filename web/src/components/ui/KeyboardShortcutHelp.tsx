import { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import { SHORTCUT_DEFINITIONS, formatHotkey, CATEGORY_LABELS, CATEGORY_ORDER } from '../../lib/hotkeys';

interface Props {
  open: boolean;
  onClose: () => void;
}

export function KeyboardShortcutHelp({ open, onClose }: Props) {
  const closeRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    closeRef.current?.focus();
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [open, onClose]);

  if (!open) return null;

  return createPortal(
    <div
      role="dialog"
      aria-modal="true"
      aria-label="Keyboard shortcuts"
      onClick={onClose}
      style={{
        position: 'fixed', inset: 0, zIndex: 9999,
        background: 'rgba(0,0,0,0.5)',
        display: 'flex', alignItems: 'center', justifyContent: 'center',
        animation: 'fadeIn 0.15s ease',
      }}
    >
      <div
        onClick={e => e.stopPropagation()}
        style={{
          background: 'var(--bg-primary)',
          border: '1px solid var(--border)',
          borderRadius: 8,
          padding: 24,
          maxWidth: 520,
          width: '90%',
          maxHeight: '80vh',
          overflow: 'auto',
          boxShadow: '0 8px 32px rgba(0,0,0,0.3)',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 20 }}>
          <h2 style={{ margin: 0, fontSize: 16, fontWeight: 600, color: 'var(--text-primary)' }}>
            Keyboard shortcuts
          </h2>
          <button
            ref={closeRef}
            onClick={onClose}
            aria-label="Close"
            style={{
              background: 'none', border: 'none', cursor: 'pointer',
              color: 'var(--text-muted)', padding: 4,
              borderRadius: 4, display: 'flex',
            }}
          >
            <X size={14} />
          </button>
        </div>

        {CATEGORY_ORDER.map(cat => {
          const items = SHORTCUT_DEFINITIONS.filter(d => d.category === cat);
          if (items.length === 0) return null;
          return (
            <div key={cat} style={{ marginBottom: 16 }}>
              <div style={{
                fontSize: 11, fontWeight: 600, textTransform: 'uppercase',
                letterSpacing: '0.05em', color: 'var(--text-muted)', marginBottom: 8,
              }}>
                {CATEGORY_LABELS[cat]}
              </div>
              {items.map((def, i) => (
                <div
                  key={i}
                  style={{
                    display: 'flex', justifyContent: 'space-between',
                    alignItems: 'center', padding: '4px 0',
                  }}
                >
                  <span style={{ fontSize: 13, color: 'var(--text-primary)' }}>
                    {def.description}
                  </span>
                  <kbd style={{
                    fontSize: 11, fontFamily: 'monospace',
                    padding: '2px 6px', borderRadius: 4,
                    background: 'var(--bg-secondary)',
                    border: '1px solid var(--border)',
                    color: 'var(--text-secondary)',
                    whiteSpace: 'nowrap',
                  }}>
                    {formatHotkey(def)}
                  </kbd>
                </div>
              ))}
            </div>
          );
        })}

        <div style={{
          marginTop: 16, paddingTop: 12, borderTop: '1px solid var(--border)',
          fontSize: 11, color: 'var(--text-muted)',
        }}>
          Press <kbd style={{ padding: '1px 4px', background: 'var(--bg-secondary)', borderRadius: 3, border: '1px solid var(--border)' }}>?</kbd> or click the button in the status bar to open this panel.
        </div>
      </div>
    </div>,
    document.getElementById('modal-root') || document.body
  );
}
