import { describe, expect, it } from 'vitest';
import { act, useRef, useState } from 'react';
import { useDismiss } from 'luvo/input/useDismiss';
import { Popover } from './Popover';
import { mount } from 'luvo/test/render';

function Harness({ open }: { open: boolean }) {
  const anchor = useRef<HTMLDivElement>(null);
  return (
    <div ref={anchor} className="picker">
      <button>open</button>
      <Popover open={open} anchor={anchor}>
        <div className="menu"><button className="menu-item">one</button></div>
      </Popover>
    </div>
  );
}

function Toggling() {
  const anchor = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  return (
    <div ref={anchor}>
      <button onClick={() => setOpen(v => !v)}>toggle</button>
      <Popover open={open} anchor={anchor}><div className="menu">shown</div></Popover>
    </div>
  );
}

describe('a popover', () => {
  /* An absolute menu inside a scrolling pane is cut by that pane's edge, and no
     z-index reaches past a clip — so it is drawn at the top of the document. */
  it('draws outside the pane it was opened from', () => {
    const ui = mount(<Harness open />);
    const pop = document.querySelector('.popover') as HTMLElement;
    expect(pop).toBeTruthy();
    expect(pop.parentElement).toBe(document.body);
    expect(pop.textContent).toContain('one');
    ui.unmount();
  });

  it('is nothing at all while it is closed', () => {
    const ui = mount(<Harness open={false} />);
    expect(document.querySelector('.popover')).toBeNull();
    ui.unmount();
  });

  it('opens and closes with the state it is given', () => {
    const ui = mount(<Toggling />);
    expect(document.querySelector('.popover')).toBeNull();
    ui.click('button');
    expect(document.querySelector('.popover')?.textContent).toBe('shown');
    ui.click('button');
    expect(document.querySelector('.popover')).toBeNull();
    ui.unmount();
  });

  it('leaves nothing behind when it goes', () => {
    const ui = mount(<Harness open />);
    ui.unmount();
    expect(document.querySelector('.popover')).toBeNull();
  });
});

describe('a popover and the click that dismisses', () => {
  /* Drawn at the top of the document, a menu is "outside" every wrapper — so a
     click on one of its own items closed it before the item was pressed. */
  it('is not an outside click for the control that opened it', () => {
    function Menu() {
      const [open, setOpen] = useState(true);
      const ref = useDismiss<HTMLDivElement>(open, () => setOpen(false));
      return (
        <div ref={ref}>
          <button>open</button>
          <Popover open={open} anchor={ref}>
            <div className="menu"><button className="menu-item">pick me</button></div>
          </Popover>
        </div>
      );
    }
    const ui = mount(<Menu />);
    const item = document.querySelector('.menu-item') as HTMLElement;
    act(() => { item.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })); });
    expect(document.querySelector('.popover')).not.toBeNull();

    act(() => { document.body.dispatchEvent(new MouseEvent('mousedown', { bubbles: true })); });
    expect(document.querySelector('.popover')).toBeNull();
    ui.unmount();
  });

  it('carries the class its caller gave it', () => {
    function Wide() {
      const anchor = useRef<HTMLDivElement>(null);
      return (
        <div ref={anchor}>
          <Popover open anchor={anchor} matchWidth className="method-menu">
            <div className="menu">wide</div>
          </Popover>
        </div>
      );
    }
    const ui = mount(<Wide />);
    const pop = document.querySelector('.popover') as HTMLElement;
    expect(pop.className).toContain('method-menu');
    ui.unmount();
  });
});
