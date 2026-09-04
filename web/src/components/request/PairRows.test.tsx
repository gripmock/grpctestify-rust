import { describe, expect, it, vi } from 'vitest';
import { PairRows } from './PairRows';
import { mount } from 'luvo/test/render';
import type { QueryParam } from '../../lib/query';

const rows = (over: Partial<QueryParam>[] = []): QueryParam[] =>
  over.map(o => ({ key: '', value: '', ...o }));

describe('rows of name and value', () => {
  it('is named for what it edits', () => {
    const ui = mount(<PairRows noun="field" rows={rows([{ key: 'a', value: '1' }])} empty="nothing yet" onChange={() => {}} />);
    expect(ui.get('input[aria-label="Field 1 name"]')).toBeTruthy();
    expect(ui.all('button').at(-1)?.textContent).toContain('field');
    ui.unmount();
  });

  it('says what stands where the rows would be', () => {
    const ui = mount(<PairRows noun="parameter" rows={[]} empty="No query parameters." onChange={() => {}} />);
    expect(ui.container.textContent).toContain('No query parameters.');
    ui.unmount();
  });

  it('shows what a row with no value will be written as, and offers the other', () => {
    const seen: QueryParam[][] = [];
    const ui = mount(
      <PairRows noun="parameter" rows={rows([{ key: 'flag' }])} empty="" onChange={next => seen.push(next)} />,
    );
    const mark = ui.get('.param-empty');
    expect(mark.textContent).toBe('flag');
    ui.click(mark);
    expect(seen[0]).toEqual([{ key: 'flag', value: '', bare: false }]);
    ui.unmount();
  });

  it('says nothing about a row that has a value', () => {
    const ui = mount(<PairRows noun="parameter" rows={rows([{ key: 'a', value: '1' }])} empty="" onChange={() => {}} />);
    expect(ui.all('.param-empty')).toEqual([]);
    ui.unmount();
  });

  it('adds and removes a row', () => {
    const onChange = vi.fn();
    const ui = mount(<PairRows noun="parameter" rows={rows([{ key: 'a', value: '1' }])} empty="" onChange={onChange} />);
    ui.click('button[aria-label="Remove parameter 1"]');
    expect(onChange).toHaveBeenCalledWith([]);
    ui.click(ui.all('button').at(-1)!);
    expect(onChange).toHaveBeenLastCalledWith([{ key: 'a', value: '1' }, { key: '', value: '' }]);
    ui.unmount();
  });
});
