import { createContext, useContext } from 'react';

export interface Choice {
  label: string;
  value: string;
  tone?: 'primary' | 'danger' | 'quiet';
}

export interface ModalApi {
  confirm: (
    title: string,
    message?: string,
    options?: { confirmText?: string; cancelText?: string; danger?: boolean },
  ) => Promise<boolean>;
  alert: (title: string, message?: string) => Promise<void>;
  prompt: (title: string, message?: string, defaultValue?: string) => Promise<string | null>;
  choose: (title: string, message: string | undefined, choices: Choice[]) => Promise<string | null>;
}

export const ModalContext = createContext<ModalApi | null>(null);

export function useModal(): ModalApi {
  const ctx = useContext(ModalContext);
  if (!ctx) throw new Error('useModal must be used within ModalProvider');
  return ctx;
}
