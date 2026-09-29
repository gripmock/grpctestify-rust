import type { RequestTab } from './types';

export function tabFills(tab: RequestTab): boolean {
  return tab === 'body' || tab === 'source';
}

export function workspaceClass(
  layout: 'rows' | 'columns',
  tab: RequestTab,
  hasOutcome: boolean,
  sized = false,
): string {
  const parts = [`workspace is-${layout}`];
  if (!hasOutcome) parts.push('is-idle');
  if (!tabFills(tab)) parts.push('is-form');
  if (sized) parts.push('is-sized');
  if (layout === 'rows' && tabFills(tab) && (sized || hasOutcome)) parts.push('is-boxed');
  return parts.join(' ');
}

export const COLUMNS_FIT = '(min-width: 64rem)';

export function columnsFit(width: number, rootPx = 16): boolean {
  return width >= 64 * rootPx;
}
