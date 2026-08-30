import { createContext, useContext, useState, useCallback, useMemo, useRef, useEffect, type ReactNode } from 'react';

type ModalType = 'confirm' | 'alert' | 'prompt' | 'choice';

export interface Choice {
  label: string;
  value: string;
  tone?: 'primary' | 'danger' | 'quiet';
}

interface ModalConfig {
  type: ModalType;
  title: string;
  message?: string;
  defaultValue?: string;
  confirmText?: string;
  cancelText?: string;
  choices?: Choice[];
  /** The action cannot be undone: it is drawn as the danger it is, and focus
   *  starts on Cancel so Enter on an unread dialog destroys nothing. */
  danger?: boolean;
}

interface ModalApi {
  confirm: (
    title: string,
    message?: string,
    options?: { confirmText?: string; cancelText?: string; danger?: boolean },
  ) => Promise<boolean>;
  alert: (title: string, message?: string) => Promise<void>;
  prompt: (title: string, message?: string, defaultValue?: string) => Promise<string | null>;
  /** More than two ways out — closing an edited file is cancel, discard *or*
      save. Dismissal resolves to null, never to one of the choices. */
  choose: (title: string, message: string | undefined, choices: Choice[]) => Promise<string | null>;
}

const ModalContext = createContext<ModalApi | null>(null);

export function useModal(): ModalApi {
  const ctx = useContext(ModalContext);
  if (!ctx) throw new Error('useModal must be used within ModalProvider');
  return ctx;
}

/** What a dismissal resolves to, per kind: cancelling a confirm is `false`,
 *  cancelling a prompt is `null`, and an alert has nothing to say. */
function dismissValue(type: ModalType) {
  if (type === 'confirm') return false;
  if (type === 'prompt' || type === 'choice') return null;
  return undefined;
}

export function ModalProvider({ children }: { children: ReactNode }) {
  const [modal, setModal] = useState<ModalConfig | null>(null);
  const [promptValue, setPromptValue] = useState('');
  const resolveRef = useRef<((value: any) => void) | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);

  /* One object for the life of the provider, for the reason the toasts have
     one: a fresh api per render is a fresh dependency for every `useCallback`
     and `useEffect` that takes it. */
  const api: ModalApi = useMemo(() => ({
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
      setPromptValue(defaultValue || '');
      setModal({ type: 'prompt', title, message, defaultValue, confirmText: 'Save', cancelText: 'Cancel' });
    }),
    choose: (title, message, choices) => new Promise(resolve => {
      resolveRef.current = resolve;
      setModal({ type: 'choice', title, message, choices, cancelText: 'Cancel' });
    }),
  }), []);

  const close = useCallback((value: any) => {
    resolveRef.current?.(value);
    resolveRef.current = null;
    setModal(null);
  }, []);

  /* A native <dialog> opened with showModal() gives us the focus trap, the
     inert page behind it, Escape and the top layer — four things the old
     hand-rolled overlay had to fake, and only managed one of. */
  useEffect(() => {
    const el = dialogRef.current;
    if (!el) return;
    if (modal && !el.open) el.showModal();
    if (!modal && el.open) el.close();
  }, [modal]);

  const handleCancel = useCallback(() => {
    if (modal) close(dismissValue(modal.type));
  }, [modal, close]);

  const handleConfirm = useCallback(() => {
    if (!modal) return;
    close(modal.type === 'prompt' ? promptValue : modal.type === 'confirm' ? true : undefined);
  }, [modal, close, promptValue]);

  return (
    <ModalContext.Provider value={api}>
      {children}
      <dialog
        ref={dialogRef}
        className="modal"
        aria-label={modal?.title}
        /* Escape and the backdrop both route through the same dismissal, so a
           promise is never left hanging. */
        onCancel={e => { e.preventDefault(); handleCancel(); }}
        onClick={e => { if (e.target === dialogRef.current) handleCancel(); }}
        /* And however else it closes: `close` resolves once and drops the
           resolver, so a second dismissal is a no-op rather than a promise
           left hanging behind a dialog nobody can reopen. */
        onClose={() => handleCancel()}
      >
        {modal && (
          <>
            <div className="modal-head">
              <h2 className="modal-title">{modal.title}</h2>
            </div>

            <div className="modal-body stack">
              {modal.message && <div className="muted">{modal.message}</div>}
              {modal.type === 'prompt' && (
                <PromptInput
                  value={promptValue}
                  onChange={setPromptValue}
                  onSubmit={v => close(v)}
                  onCancel={() => close(null)}
                />
              )}
            </div>

            <div className="modal-foot">
              {modal.type === 'choice' ? (
                <>
                  <button className="btn is-quiet" onClick={handleCancel}>
                    {modal.cancelText || 'Cancel'}
                  </button>
                  {(modal.choices || []).map((c, i) => (
                    <button
                      key={c.value}
                      className={`btn${c.tone === 'primary' ? ' is-primary' : c.tone === 'danger' ? ' is-danger' : c.tone === 'quiet' ? ' is-quiet' : ''}`}
                      onClick={() => close(c.value)}
                      autoFocus={i === (modal.choices || []).length - 1}
                    >
                      {c.label}
                    </button>
                  ))}
                </>
              ) : (
              <>
              {modal.type !== 'alert' && (
                <button className="btn is-quiet" onClick={handleCancel} autoFocus={modal.danger}>
                  {modal.cancelText || 'Cancel'}
                </button>
              )}
              {/* A delete used to be drawn as the ordinary primary and hold the
                  focus, so Enter on a dialog nobody had read went through. */}
              <button
                className={`btn is-primary${modal.danger ? ' is-danger' : ''}`}
                onClick={handleConfirm}
                autoFocus={!modal.danger}
              >
                {modal.confirmText || 'Confirm'}
              </button>
              </>
              )}
            </div>
          </>
        )}
      </dialog>
    </ModalContext.Provider>
  );
}

function PromptInput({ value, onChange, onSubmit, onCancel }: { value: string; onChange: (v: string) => void; onSubmit: (v: string) => void; onCancel: () => void }) {
  const ref = useRef<HTMLInputElement>(null);
  useEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  return (
    <input
      ref={ref}
      className="field mono"
      value={value}
      onChange={e => onChange(e.target.value)}
      onKeyDown={e => {
        if (e.key === 'Enter') onSubmit(value);
        /* Escape is the dialog's own; stop it here so the input's handler and
           the dialog's onCancel do not both resolve the promise. */
        if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); onCancel(); }
      }}
    />
  );
}
