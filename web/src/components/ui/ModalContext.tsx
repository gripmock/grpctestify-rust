import { createContext, useContext, useState, useCallback, useRef, useEffect, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

type ModalType = 'confirm' | 'alert' | 'prompt';

interface ModalConfig {
  type: ModalType;
  title: string;
  message?: string;
  defaultValue?: string;
  confirmText?: string;
  cancelText?: string;
}

interface ModalApi {
  confirm: (title: string, message?: string, options?: { confirmText?: string; cancelText?: string }) => Promise<boolean>;
  alert: (title: string, message?: string) => Promise<void>;
  prompt: (title: string, message?: string, defaultValue?: string) => Promise<string | null>;
}

const ModalContext = createContext<ModalApi | null>(null);

export function useModal(): ModalApi {
  const ctx = useContext(ModalContext);
  if (!ctx) throw new Error('useModal must be used within ModalProvider');
  return ctx;
}

export function ModalProvider({ children }: { children: ReactNode }) {
  const [modal, setModal] = useState<ModalConfig | null>(null);
  const resolveRef = useRef<((value: any) => void) | null>(null);

  const api: ModalApi = {
    confirm: (title, message, options) => new Promise(resolve => {
      resolveRef.current = resolve;
      setModal({ type: 'confirm', title, message, confirmText: 'Confirm', cancelText: 'Cancel', ...options });
    }),
    alert: (title, message) => new Promise(resolve => {
      resolveRef.current = resolve;
      setModal({ type: 'alert', title, message, confirmText: 'OK' });
    }),
    prompt: (title, message, defaultValue) => new Promise(resolve => {
      resolveRef.current = resolve;
      setModal({ type: 'prompt', title, message, defaultValue, confirmText: 'Save', cancelText: 'Cancel' });
    }),
  };

  const close = useCallback((value: any) => {
    resolveRef.current?.(value);
    resolveRef.current = null;
    setModal(null);
  }, []);

  const handleOverlayClick = useCallback(() => {
    if (modal?.type === 'confirm') close(false);
    else if (modal?.type === 'alert') close(undefined);
    else if (modal?.type === 'prompt') close(null);
  }, [modal?.type, close]);

  const handleCancel = useCallback(() => {
    if (modal?.type === 'confirm') close(false);
    else if (modal?.type === 'alert') close(undefined);
    else if (modal?.type === 'prompt') close(null);
  }, [modal?.type, close]);

  const handleConfirm = useCallback(() => {
    if (modal?.type === 'confirm') close(true);
    else if (modal?.type === 'alert') close(undefined);
  }, [modal?.type, close]);

  const overlayRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (modal) {
      const el = overlayRef.current;
      if (el) {
        const target = el.querySelector<HTMLElement>('button, input, [tabindex]:not([tabindex="-1"])');
        target?.focus();
      }
    }
  }, [modal]);

  return (
    <ModalContext.Provider value={api}>
      {children}
      {modal && createPortal(
        <div ref={overlayRef}
          onKeyDown={e => {
            if (e.key === 'Escape') handleCancel();
          }}
          style={{
            position: 'fixed', inset: 0, zIndex: 10000,
            display: 'flex', alignItems: 'center', justifyContent: 'center',
            background: 'rgba(0,0,0,0.5)',
          }}
          onClick={handleOverlayClick}
        >
          <div onClick={e => e.stopPropagation()}
            style={{
              background: 'var(--bg-primary)', borderRadius: 10,
              border: '1px solid var(--border)', boxShadow: '0 8px 32px rgba(0,0,0,0.3)',
              width: 380, maxWidth: '90vw',
            }}
          >
            <div style={{ padding: '16px 20px', borderBottom: '1px solid var(--border)' }}>
              <div style={{ fontSize: 14, fontWeight: 600 }}>{modal.title}</div>
            </div>

            <div style={{ padding: '12px 20px' }}>
              {modal.message && (
                <div style={{ fontSize: 13, color: 'var(--text-secondary)' }}>{modal.message}</div>
              )}
              {modal.type === 'prompt' && (
                <PromptInput
                  defaultValue={modal.defaultValue || ''}
                  onSubmit={v => close(v)}
                  onCancel={() => close(null)}
                />
              )}
            </div>

            <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 6, padding: '12px 20px', borderTop: '1px solid var(--border)' }}>
              {modal.type !== 'alert' && (
                <button onClick={handleCancel} style={{
                  padding: '6px 14px', fontSize: 12, borderRadius: 6, cursor: 'pointer',
                  border: '1px solid var(--border)', background: 'var(--bg-primary)',
                  color: 'var(--text-primary)',
                }}>
                  {modal.cancelText || 'Cancel'}
                </button>
              )}
              <button onClick={handleConfirm} style={{
                padding: '6px 14px', fontSize: 12, borderRadius: 6, cursor: 'pointer',
                border: 'none', background: 'var(--accent)', color: '#fff',
              }}>
                {modal.confirmText || 'Confirm'}
              </button>
            </div>
          </div>
        </div>,
        document.getElementById('modal-root') || document.body
      )}
    </ModalContext.Provider>
  );
}

function PromptInput({ defaultValue, onSubmit, onCancel }: { defaultValue: string; onSubmit: (v: string) => void; onCancel: () => void }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (ref.current) {
      ref.current.focus();
      ref.current.select();
    }
  }, []);
  return (
    <input ref={ref} defaultValue={defaultValue}
      onKeyDown={e => {
        if (e.key === 'Enter') onSubmit(ref.current?.value ?? '');
        if (e.key === 'Escape') onCancel();
      }}
      style={{
        width: '100%', padding: '8px 10px', fontSize: 13, borderRadius: 6, marginTop: 8,
        border: '1px solid var(--border)', background: 'var(--bg-primary)',
        color: 'var(--text-primary)', outline: 'none', boxSizing: 'border-box',
        fontFamily: 'monospace',
      }}
    />
  );
}
