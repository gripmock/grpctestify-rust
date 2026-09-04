import { describe, expect, it, vi } from 'vitest';
import { ContextMenu } from './ContextMenu';
import { mount } from 'luvo/test/render';

const menu = (onClose: () => void) => (
  <ContextMenu at={{ x: 10, y: 10 }} onClose={onClose} className="rail-menu" label="This file">
    <button className="menu-item">open</button>
    <button className="menu-item">rename</button>
  </ContextMenu>
);

describe('a menu opened at a point', () => {
  it('is a menu the keyboard lands in', () => {
    const ui = mount(menu(() => {}));
    expect(ui.get('[role="menu"]').getAttribute('aria-label')).toBe('This file');
    expect(document.activeElement?.textContent).toBe('open');
    ui.key(document.activeElement!, 'ArrowDown');
    expect(document.activeElement?.textContent).toBe('rename');
    ui.unmount();
  });

  it('closes on Escape', () => {
    const onClose = vi.fn();
    const ui = mount(menu(onClose));
    ui.key(document.activeElement!, 'Escape');
    expect(onClose).toHaveBeenCalledTimes(1);
    ui.unmount();
  });

  it('carries the class its caller gave it', () => {
    const ui = mount(menu(() => {}));
    expect(ui.get('[role="menu"]').className).toBe('menu is-floating rail-menu');
    ui.unmount();
  });
});
