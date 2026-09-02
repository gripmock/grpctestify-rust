import { describe, expect, it } from 'vitest';
import { ThemePicker } from './ThemePicker';
import { mount } from 'luvo/test/render';

describe('the palette menu', () => {
  it('lands on the first palette, walks with the arrows and closes on Escape', () => {
    const ui = mount(<ThemePicker />);
    const trigger = ui.get('.theme-more');
    trigger.focus();
    ui.click(trigger);
    const first = document.activeElement as HTMLElement;
    expect(first.getAttribute('role')).toBe('menuitemradio');
    ui.key(first, 'ArrowDown');
    expect(document.activeElement).not.toBe(first);
    expect(document.activeElement?.getAttribute('role')).toBe('menuitemradio');
    ui.key(document.activeElement!, 'Escape');
    expect(document.querySelector('[role="menu"]')).toBeNull();
    expect(document.activeElement).toBe(trigger);
    ui.unmount();
  });
});
