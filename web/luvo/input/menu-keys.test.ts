import { describe, expect, it } from 'vitest';
import { menuMove, ownsKey } from 'luvo/input/menu-keys';

describe('where a key moves inside a menu', () => {
  it('walks and wraps like a tab strip', () => {
    expect(menuMove(0, 3, 'ArrowDown')).toBe(1);
    expect(menuMove(2, 3, 'ArrowDown')).toBe(0);
    expect(menuMove(0, 3, 'ArrowUp')).toBe(2);
    expect(menuMove(1, 3, 'Home')).toBe(0);
    expect(menuMove(1, 3, 'End')).toBe(2);
  });

  it('enters from the nearest end when nothing inside has focus', () => {
    expect(menuMove(-1, 3, 'ArrowDown')).toBe(0);
    expect(menuMove(-1, 3, 'Home')).toBe(0);
    expect(menuMove(-1, 3, 'ArrowUp')).toBe(2);
    expect(menuMove(-1, 3, 'End')).toBe(2);
  });

  it('leaves other keys alone, and an empty menu too', () => {
    expect(menuMove(1, 3, 'Enter')).toBeNull();
    expect(menuMove(-1, 3, 'a')).toBeNull();
    expect(menuMove(0, 0, 'ArrowDown')).toBeNull();
  });
});

describe('which keys a menu answers', () => {
  it('answers keys on its items and on itself, not on a control it wraps', () => {
    const menu = document.createElement('div');
    menu.innerHTML = '<button role="menuitem"></button><button class="menu-item"></button><div role="radiogroup"><button role="radio"></button></div>';
    const [item, plain, radio] = [...menu.querySelectorAll('button')];
    expect(ownsKey(item, menu)).toBe(true);
    expect(ownsKey(plain, menu)).toBe(true);
    expect(ownsKey(menu, menu)).toBe(true);
    expect(ownsKey(null, menu)).toBe(true);
    expect(ownsKey(radio, menu)).toBe(false);
  });
});
