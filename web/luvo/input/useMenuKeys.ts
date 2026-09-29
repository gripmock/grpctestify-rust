import { useCallback, useEffect, useRef, type KeyboardEvent, type RefObject } from 'react';
import { MENU_ITEMS, menuMove, ownsKey } from 'luvo/input/menu-keys';

export function menuItems(menu: HTMLElement): HTMLElement[] {
  return [...menu.querySelectorAll<HTMLElement>(MENU_ITEMS)];
}

function land(items: HTMLElement[], at: number): boolean {
  items.forEach((item, i) => { item.tabIndex = i === at ? 0 : -1; });
  items[at]?.focus();
  return document.activeElement === items[at];
}

export type MenuKeys<T extends HTMLElement> = [RefObject<T | null>, (e: KeyboardEvent) => void];

export function useMenuKeys<T extends HTMLElement>(open: boolean | string | null, close: () => void): MenuKeys<T> {
  const ref = useRef<T>(null);
  const opener = useRef<HTMLElement | null>(null);

  useEffect(() => {
    if (!open) return;
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const opened = ref.current;
    const enter = () => {
      const menu = ref.current;
      if (!menu) return false;
      const items = menuItems(menu);
      return items.length > 0 && land(items, 0);
    };
    const later = enter() ? null : window.setTimeout(enter, 0);
    return () => {
      if (later !== null) window.clearTimeout(later);
      const back = opener.current;
      const focus = document.activeElement;
      const lost = focus === null || focus === document.body || !!opened?.contains(focus);
      if (back && back.isConnected && lost) back.focus();
    };
  }, [open]);

  const onKeyDown = useCallback((e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      close();
      return;
    }
    const menu = ref.current;
    if (!menu) return;
    const target = e.target instanceof Element ? e.target : null;
    if (!ownsKey(target, menu)) return;
    const items = menuItems(menu);
    const next = menuMove(items.findIndex(item => item === target), items.length, e.key);
    if (next === null) return;
    e.preventDefault();
    land(items, next);
  }, [close]);

  return [ref, onKeyDown];
}
