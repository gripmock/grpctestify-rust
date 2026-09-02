import { nextTabIndex } from 'luvo/input/tab-keys';

export const MENU_ITEMS = '[role^="menuitem"]:not(:disabled), .menu-item:not(:disabled)';

export function menuMove(at: number, count: number, key: string): number | null {
  if (count === 0) return null;
  if (at >= 0) return nextTabIndex(at, count, key);
  switch (key) {
    case 'ArrowUp':
    case 'ArrowLeft':
    case 'End':
      return count - 1;
    case 'ArrowDown':
    case 'ArrowRight':
    case 'Home':
      return 0;
    default:
      return null;
  }
}

export function ownsKey(target: Element | null, menu: Element): boolean {
  if (target === null || target === menu) return true;
  return target.matches('[role^="menuitem"], .menu-item');
}
