import { describe, expect, it, vi } from 'vitest';
import { Splitter } from './Splitter';
import { mount } from 'luvo/test/render';

const props = {
  className: 'hsplit',
  orientation: 'horizontal' as const,
  value: 380,
  min: 220,
  max: 900,
  label: 'Request pane height',
};

describe('a splitter', () => {
  it('announces where it is', () => {
    const ui = mount(<Splitter {...props} onValue={() => {}} />);
    const handle = ui.get('[role="separator"]');
    expect(handle.getAttribute('aria-valuenow')).toBe('380');
    expect(handle.getAttribute('aria-valuemin')).toBe('220');
    expect(handle.getAttribute('aria-valuemax')).toBe('900');
    expect(handle.getAttribute('aria-orientation')).toBe('horizontal');
    expect(handle.tabIndex).toBe(0);
    ui.unmount();
  });

  it('nudges with an arrow and moves further with Shift', () => {
    const onValue = vi.fn();
    const ui = mount(<Splitter {...props} step={16} onValue={onValue} />);
    ui.key('[role="separator"]', 'ArrowDown');
    expect(onValue).toHaveBeenLastCalledWith(396);
    ui.key('[role="separator"]', 'ArrowDown', { shiftKey: true });
    expect(onValue).toHaveBeenLastCalledWith(380 + 64);
    ui.unmount();
  });

  it('takes Home and End to its limits', () => {
    const onValue = vi.fn();
    const ui = mount(<Splitter {...props} onValue={onValue} />);
    ui.key('[role="separator"]', 'Home');
    expect(onValue).toHaveBeenLastCalledWith(220);
    ui.key('[role="separator"]', 'End');
    expect(onValue).toHaveBeenLastCalledWith(900);
    ui.unmount();
  });

  it('never moves past its limits', () => {
    const onValue = vi.fn();
    const ui = mount(<Splitter {...props} value={225} step={100} onValue={onValue} />);
    ui.key('[role="separator"]', 'ArrowUp');
    expect(onValue).toHaveBeenLastCalledWith(220);
    ui.unmount();
  });

  it('reads the arrows the way the pane grows', () => {
    const onValue = vi.fn();
    const ui = mount(<Splitter {...props} invert step={16} onValue={onValue} />);
    ui.key('[role="separator"]', 'ArrowUp');
    expect(onValue).toHaveBeenLastCalledWith(396);
    ui.unmount();
  });

  it('ignores a key that is not a move', () => {
    const onValue = vi.fn();
    const ui = mount(<Splitter {...props} onValue={onValue} />);
    ui.key('[role="separator"]', 'Enter');
    expect(onValue).not.toHaveBeenCalled();
    ui.unmount();
  });
});
