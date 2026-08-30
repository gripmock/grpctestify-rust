import { createContext, useContext, useState, useCallback, useMemo, useRef, type ReactNode } from 'react';
import { createPortal } from 'react-dom';
import { X } from 'lucide-react';
import { REFUSAL_TYPE, keepLast, repeatsNewest, toastLife } from 'luvo/ui/toast-life';

import type { ToastType } from 'luvo/ui/toast-life';

interface Toast {
  id: number;
  type: ToastType;
  message: string;
}

interface ToastApi {
  success: (message: string) => void;
  error: (message: string) => void;
  /** Not a failure, and not over in four seconds: something the next click
   *  depends on having been read. */
  warn: (message: string) => void;
  info: (message: string) => void;
  /** Nothing was attempted and nothing went wrong: the state does not allow
   *  this, and the same click will say so again. */
  refuse: (message: string) => void;
}

const ToastContext = createContext<ToastApi | null>(null);

export function useToast(): ToastApi {
  const ctx = useContext(ToastContext);
  if (!ctx) throw new Error('useToast must be used within ToastProvider');
  return ctx;
}

const TOAST_CLASS: Record<ToastType, string> = {
  success: 'is-ok',
  error: 'is-fail',
  warn: 'is-warn',
  info: 'is-info',
};

/* `↻` reads as work in progress, which an info toast never is: it is something
   that has already happened, or a fact about the state. */
const TOAST_MARK: Record<string, string> = { success: '✓', error: '✗', warn: '⚠', info: 'ℹ' };

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(0);

  const addToast = useCallback((type: ToastType, message: string) => {
    const id = nextId.current++;
    let added = true;
    setToasts(prev => {
      /* The same words under the same words: a condition that keeps failing —
         a workbench checked every fifteen seconds — used to stack its refusal
         until the cap. */
      if (repeatsNewest(prev, { type, message })) { added = false; return prev; }
      return keepLast([...prev, { id, type, message }]);
    });
    if (!added) return;
    const life = toastLife(type);
    /* A refusal stays until it is closed — the one kind worth reading twice was
       also the one that left while it was being read. */
    if (life === null) return;
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id));
    }, life);
  }, []);

  const removeToast = useCallback((id: number) => {
    setToasts(prev => prev.filter(t => t.id !== id));
  }, []);

  /* One object for the life of the provider. A fresh one per render is a fresh
     dependency for every `useCallback`/`useEffect` that takes it — which, in a
     workbench whose panes re-render on every keystroke, meant timers being torn
     down and re-armed continuously. */
  const api: ToastApi = useMemo(() => ({
    success: (message) => addToast('success', message),
    error: (message) => addToast('error', message),
    warn: (message) => addToast('warn', message),
    info: (message) => addToast('info', message),
    refuse: (message) => addToast(REFUSAL_TYPE, message),
  }), [addToast]);

  return (
    <ToastContext.Provider value={api}>
      {children}
      {createPortal(
        <div className="toasts">
          {toasts.map(t => (
            <div
              key={t.id}
              className={`toast ${TOAST_CLASS[t.type]}`}
              role="status"
            >
              {/* A mark, not only a colour: the kind survives a screenshot in
                  greyscale and a reader who cannot separate the hues. */}
              <span className="toast-mark">{TOAST_MARK[t.type]}</span>
              <span className="toast-text">{t.message}</span>
              {/* Closing is a button, not the whole surface: selecting the text
                  of an error used to dismiss it mid-selection. */}
              <button
                className="btn is-ghost is-icon is-sm"
                onClick={() => removeToast(t.id)}
                aria-label="Dismiss"
                title="Dismiss"
              >
                <X size={11} />
              </button>
            </div>
          ))}
        </div>,
        document.getElementById('toast-root') || document.body
      )}
    </ToastContext.Provider>
  );
}
