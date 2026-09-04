import type { CallResult } from './types';

export function serverAnswered(response: CallResult | null | undefined): boolean {
  if (!response || response.status === 'pending') return false;
  if (response.sent === false) return false;
  return response.statusCode !== null || response.messages.length > 0;
}

export const NOTHING_TO_EXPECT = 'Nothing to expect — the call never reached a server';
