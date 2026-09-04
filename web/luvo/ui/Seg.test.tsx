import { describe, expect, it, vi } from 'vitest';
import { Seg } from './Seg';
import { mount } from 'luvo/test/render';

const OPTIONS = [
  { value: 'a' as const, label: 'first' },
  { value: 'b' as const, label: 'second' },
  { value: 'c' as const, label: 'third' },
];

describe('a segmented choice', () => {
  it('says which one is chosen', () => {
    const ui = mount(<Seg label="Which" value="b" onChange={() => {}} options={OPTIONS} />);
    expect(ui.get('[role="radiogroup"]')?.getAttribute('aria-label')).toBe('Which');
    expect(ui.all('[role="radio"]').map(r => r.getAttribute('aria-checked'))).toEqual(['false', 'true', 'false']);
    ui.unmount();
  });

  it('keeps one tab stop for the group', () => {
    const ui = mount(<Seg label="Which" value="b" onChange={() => {}} options={OPTIONS} />);
    expect(ui.all('[role="radio"]').map(r => r.tabIndex)).toEqual([-1, 0, -1]);
    ui.unmount();
  });

  it('stays reachable when nothing is chosen', () => {
    const ui = mount(<Seg label="Which" value={null} onChange={() => {}} options={OPTIONS} />);
    expect(ui.all('[role="radio"]').map(r => r.tabIndex)).toEqual([0, -1, -1]);
    ui.unmount();
  });

  it('walks and chooses with the arrows', () => {
    const onChange = vi.fn();
    const ui = mount(<Seg label="Which" value="b" onChange={onChange} options={OPTIONS} />);
    ui.key('[role="radiogroup"]', 'ArrowRight');
    expect(onChange).toHaveBeenCalledWith('c');
    ui.key('[role="radiogroup"]', 'Home');
    expect(onChange).toHaveBeenLastCalledWith('a');
    ui.unmount();
  });

  it('leaves a key it does not use alone', () => {
    const onChange = vi.fn();
    const ui = mount(<Seg label="Which" value="b" onChange={onChange} options={OPTIONS} />);
    ui.key('[role="radiogroup"]', 'q');
    expect(onChange).not.toHaveBeenCalled();
    ui.unmount();
  });

  it('will not choose an option that is refused', () => {
    const onChange = vi.fn();
    const options = [OPTIONS[0], { value: 'b' as const, label: 'second', disabled: true }];
    const ui = mount(<Seg label="Which" value="a" onChange={onChange} options={options} />);
    ui.key('[role="radiogroup"]', 'ArrowRight');
    expect(onChange).not.toHaveBeenCalled();
    expect(ui.all('[role="radio"]')[1].hasAttribute('disabled')).toBe(true);
    ui.unmount();
  });
});
