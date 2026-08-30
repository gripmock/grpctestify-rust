import { describe, expect, it, vi } from 'vitest';
import { Tabs } from './Tabs';
import { mount } from 'luvo/test/render';

const ITEMS = [
  { key: 'a' as const, label: 'first' },
  { key: 'b' as const, label: 'second' },
  { key: 'c' as const, label: 'third' },
];

describe('a tab strip', () => {
  it('says which tab is the current one', () => {
    const ui = mount(<Tabs items={ITEMS} value="b" onChange={() => {}} />);
    const tabs = ui.all('[role="tab"]');
    expect(tabs).toHaveLength(3);
    expect(tabs.map(t => t.getAttribute('aria-selected'))).toEqual(['false', 'true', 'false']);
    expect(ui.get('[role="tablist"]')).toBeTruthy();
    ui.unmount();
  });

  it('keeps one tab stop for the strip', () => {
    const ui = mount(<Tabs items={ITEMS} value="b" onChange={() => {}} />);
    expect(ui.all('[role="tab"]').map(t => t.tabIndex)).toEqual([-1, 0, -1]);
    ui.unmount();
  });

  it('walks with the arrows', () => {
    const onChange = vi.fn();
    const ui = mount(<Tabs items={ITEMS} value="b" onChange={onChange} />);
    ui.key('[role="tablist"]', 'ArrowRight');
    expect(onChange).toHaveBeenCalledWith('c');
    ui.key('[role="tablist"]', 'ArrowLeft');
    expect(onChange).toHaveBeenLastCalledWith('a');
    ui.unmount();
  });

  it('takes Home and End to the ends', () => {
    const onChange = vi.fn();
    const ui = mount(<Tabs items={ITEMS} value="b" onChange={onChange} />);
    ui.key('[role="tablist"]', 'Home');
    expect(onChange).toHaveBeenLastCalledWith('a');
    ui.key('[role="tablist"]', 'End');
    expect(onChange).toHaveBeenLastCalledWith('c');
    ui.unmount();
  });

  it('ignores a key that means nothing to it', () => {
    const onChange = vi.fn();
    const ui = mount(<Tabs items={ITEMS} value="b" onChange={onChange} />);
    ui.key('[role="tablist"]', 'q');
    expect(onChange).not.toHaveBeenCalled();
    ui.unmount();
  });

  it('picks the tab that was clicked', () => {
    const onChange = vi.fn();
    const ui = mount(<Tabs items={ITEMS} value="a" onChange={onChange} />);
    ui.click(ui.all('[role="tab"]')[2]);
    expect(onChange).toHaveBeenCalledWith('c');
    ui.unmount();
  });
});

describe('a strip with a name', () => {
  it('says what it is', () => {
    const ui = mount(<Tabs items={ITEMS} value="a" onChange={() => {}} label="Sections of this request" />);
    expect(ui.get('[role="tablist"]')?.getAttribute('aria-label')).toBe('Sections of this request');
    ui.unmount();
  });
});
