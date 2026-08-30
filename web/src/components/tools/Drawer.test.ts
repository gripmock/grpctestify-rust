import { describe, expect, it } from 'vitest';
import { drawerHeight } from './Drawer';

describe('what the drawer may take of the window', () => {
  it('keeps what was dragged while there is room', () => {
    expect(drawerHeight(360, 900)).toBe(360);
    expect(drawerHeight(500, 1200)).toBe(500);
  });

  it('never more than half of a short window', () => {
    expect(drawerHeight(360, 577)).toBe(289);
    expect(drawerHeight(620, 700)).toBe(350);
  });

  it('never below its own floor, however short the window', () => {
    expect(drawerHeight(360, 400)).toBe(260);
    expect(drawerHeight(260, 300)).toBe(260);
  });

  it('never above its own ceiling, however tall', () => {
    expect(drawerHeight(900, 2000)).toBe(620);
  });
});
