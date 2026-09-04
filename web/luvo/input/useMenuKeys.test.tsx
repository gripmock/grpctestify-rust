import { describe, expect, it, vi } from 'vitest';
import { useState } from 'react';
import { useMenuKeys } from 'luvo/input/useMenuKeys';
import { mount } from 'luvo/test/render';

function Picker({ onClose }: { onClose?: () => void }) {
  const [open, setOpen] = useState(false);
  const [menuRef, onMenuKeys] = useMenuKeys<HTMLDivElement>(open, () => { setOpen(false); onClose?.(); });
  return (
    <div>
      <button className="trigger" onClick={() => setOpen(v => !v)}>open</button>
      {open && (
        <div ref={menuRef} role="menu" onKeyDown={onMenuKeys}>
          <button role="menuitem" className="menu-item">one</button>
          <button role="menuitem" className="menu-item" disabled>skipped</button>
          <button role="menuitem" className="menu-item">two</button>
          <button className="menu-item">three</button>
          <div role="radiogroup"><button role="radio" className="radio">a</button></div>
        </div>
      )}
    </div>
  );
}

const focused = () => document.activeElement?.textContent;

describe('a menu and the keyboard', () => {
  it('lands on the first item when it opens', () => {
    const ui = mount(<Picker />);
    (ui.get('.trigger') as HTMLElement).focus();
    ui.click('.trigger');
    expect(focused()).toBe('one');
    ui.unmount();
  });

  it('walks with the arrows, skipping what is disabled, and wraps', () => {
    const ui = mount(<Picker />);
    ui.click('.trigger');
    ui.key(document.activeElement!, 'ArrowDown');
    expect(focused()).toBe('two');
    ui.key(document.activeElement!, 'ArrowDown');
    expect(focused()).toBe('three');
    ui.key(document.activeElement!, 'ArrowDown');
    expect(focused()).toBe('one');
    ui.key(document.activeElement!, 'ArrowUp');
    expect(focused()).toBe('three');
    ui.unmount();
  });

  it('takes Home and End to the ends', () => {
    const ui = mount(<Picker />);
    ui.click('.trigger');
    ui.key(document.activeElement!, 'End');
    expect(focused()).toBe('three');
    ui.key(document.activeElement!, 'Home');
    expect(focused()).toBe('one');
    ui.unmount();
  });

  it('keeps one tab stop, on the item that has focus', () => {
    const ui = mount(<Picker />);
    ui.click('.trigger');
    ui.key(document.activeElement!, 'ArrowDown');
    expect(ui.all('[role="menuitem"]:not(:disabled)').map(b => b.tabIndex)).toEqual([-1, 0]);
    ui.unmount();
  });

  it('closes on Escape and hands focus back to what opened it', () => {
    const onClose = vi.fn();
    const ui = mount(<Picker onClose={onClose} />);
    const trigger = ui.get('.trigger');
    trigger.focus();
    ui.click('.trigger');
    expect(focused()).toBe('one');
    ui.key(document.activeElement!, 'Escape');
    expect(onClose).toHaveBeenCalledTimes(1);
    expect(ui.all('[role="menu"]')).toEqual([]);
    expect(document.activeElement).toBe(trigger);
    ui.unmount();
  });

  it('leaves the arrows to a control the menu wraps', () => {
    const ui = mount(<Picker />);
    ui.click('.trigger');
    const radio = ui.get('.radio');
    radio.focus();
    ui.key(radio, 'ArrowDown');
    expect(document.activeElement).toBe(radio);
    ui.unmount();
  });

  it('does not steal focus back when the pointer went somewhere else', () => {
    const ui = mount(<Picker />);
    const trigger = ui.get('.trigger');
    trigger.focus();
    ui.click('.trigger');
    const elsewhere = document.createElement('button');
    document.body.appendChild(elsewhere);
    elsewhere.focus();
    ui.click('.trigger');
    expect(document.activeElement).toBe(elsewhere);
    elsewhere.remove();
    ui.unmount();
  });
});
