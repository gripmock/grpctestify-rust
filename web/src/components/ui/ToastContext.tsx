import { createContext, useContext, useState, useCallback, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';

type ToastType = 'success' | 'error' | 'info';

interface Toast {
  id: number;
  type: ToastType;
  message: string;
}

interface ToastApi {
  success: (message: string) => void;
  error: (message: string) => void;
  info: (message: string) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

export function useToast(): ToastApi {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error('useToast must be used within ToastProvider');
  return ctx;
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(0);

  const addToast = useCallback((type: ToastType, message: string) => {
    const id = nextId.current++;
    setToasts(prev => [...prev, { id, type, message }]);
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id));
    }, 4000);
  }, []);

  const removeToast = useCallback((id: number) => {
    setToasts(prev => prev.filter(t => t.id !== id));
  }, []);

  const api: ToastApi = {
    success: (message) => addToast('success', message),
    error: (message) => addToast('error', message),
    info: (message) => addToast('info', message),
  };

  const typeColors: Record<ToastType, { bg: string; border: string; accent: string }> = {
    success: { bg: '#052e16', border: '#166534', accent: '#22c55e' },
    error: { bg: '#450a0a', border: '#991b1b', accent: '#ef4444' },
    info: { bg: '#0f172a', border: '#334155', accent: '#3b82f6' },
  };

  return (
    <ToastContext.Provider value={api}>
      {children}
      {createPortal(
        <div style={{
          position: 'fixed', bottom: 16, right: 16, zIndex: 10001,
          display: 'flex', flexDirection: 'column', gap: 6, pointerEvents: 'none',
        }}>
          {toasts.map(t => {
            const c = typeColors[t.type];
            return (
              <div key={t.id} onClick={() => removeToast(t.id)}
                style={{
                  pointerEvents: 'auto', cursor: 'pointer',
                  display: 'flex', alignItems: 'center', gap: 8,
                  padding: '10px 14px', borderRadius: 8,
                  background: c.bg, border: `1px solid ${c.border}`,
                  boxShadow: '0 4px 12px rgba(0,0,0,0.3)',
                  fontSize: 12, color: '#fff', maxWidth: 360,
                  animation: 'toast-in 0.2s ease',
                }}
              >
                <span style={{ width: 6, height: 6, borderRadius: '50%', background: c.accent, flexShrink: 0 }} />
                {t.message}
              </div>
            );
          })}
        </div>,
        document.getElementById('toast-root') || document.body
      )}
    </ToastContext.Provider>
  );
}
