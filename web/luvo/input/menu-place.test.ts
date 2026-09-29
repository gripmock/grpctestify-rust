import { describe, it, expect } from 'vitest';
import { placeMenu } from 'luvo/input/menu-place';

const view = { width: 1000, height: 800 };
const box = { width: 200, height: 300 };

describe('placeMenu', () => {
  it('opens at the point when it fits there', () => {
    expect(placeMenu({ x: 100, y: 100 }, box, view)).toEqual({ left: 100, top: 100 });
  });

  it('pulls a menu back inside the right edge', () => {
    expect(placeMenu({ x: 950, y: 100 }, box, view).left).toBe(792);
  });

  /* The last item of a menu opened near the bottom of a long rail used to be
     off-screen, which is where `Delete` lives. */
  it('flips above the point when there is no room below', () => {
    expect(placeMenu({ x: 100, y: 700 }, box, view).top).toBe(400);
  });

  it('keeps a menu taller than the window on screen at all', () => {
    expect(placeMenu({ x: 10, y: 700 }, { width: 200, height: 900 }, view)).toEqual({ left: 10, top: 8 });
  });
});
