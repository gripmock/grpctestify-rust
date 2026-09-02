import { useCallback, useState } from 'react';

export interface IntentFlag {
  open: boolean;
  seen: number;
}

export function intentShows(flag: IntentFlag, intent: number): boolean {
  return flag.open || flag.seen !== intent;
}

export function useIntentFlag(intent: number): [boolean, (open: boolean) => void] {
  const [flag, setFlag] = useState<IntentFlag>(() => ({ open: false, seen: intent }));
  const set = useCallback((open: boolean) => setFlag({ open, seen: intent }), [intent]);
  return [intentShows(flag, intent), set];
}

export function useIntentText(intent: number, fallback: string): [string, (text: string) => void] {
  const [typed, setTyped] = useState<{ intent: number; text: string } | null>(null);
  const set = useCallback((text: string) => setTyped({ intent, text }), [intent]);
  return [typed !== null && typed.intent === intent ? typed.text : fallback, set];
}
