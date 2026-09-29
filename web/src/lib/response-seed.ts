import type { CallResult } from './types';

export function seedIndex(response: CallResult | null, selected: number): number {
  const count = response?.messages.length ?? 0;
  if (count === 0) return 0;
  return Math.min(Math.max(0, selected), count - 1);
}

export function answered(response: CallResult | null | undefined): boolean {
  return !!response && response.status !== 'pending' && response.messages.length > 0;
}

export function seedMessage(response: CallResult | null, selected: number): unknown | null {
  if (!answered(response) || !response) return null;
  return response.messages[seedIndex(response, selected)] ?? null;
}

export function seedLabel(response: CallResult | null, selected: number): string | null {
  const count = response?.messages.length ?? 0;
  if (count < 2) return null;
  return `message ${seedIndex(response, selected) + 1} of ${count}`;
}
