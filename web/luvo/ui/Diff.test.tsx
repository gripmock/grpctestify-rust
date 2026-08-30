import { describe, expect, it } from 'vitest';
import { Diff } from './Diff';
import { lineDiff } from 'luvo/data/diff';
import { mount } from 'luvo/test/render';

describe('two versions of a text', () => {
  it('marks each line with a character, not only a colour', () => {
    const ui = mount(<Diff lines={lineDiff('a\nb', 'a\nc')} />);
    expect(ui.all('.diff-mark').map(m => m.textContent)).toEqual(['  ', '- ', '+ ']);
    ui.unmount();
  });

  it('copies as a diff reads', () => {
    const ui = mount(<Diff lines={lineDiff('keep\ngone', 'keep\nnew')} />);
    expect(ui.all('.diff > span').map(l => l.textContent)).toEqual([
      '  keep',
      '- gone',
      '+ new',
    ]);
    ui.unmount();
  });

  it('keeps an empty line a line', () => {
    const ui = mount(<Diff lines={lineDiff('a\n\nb', 'a\n\nb')} />);
    expect(ui.all('.diff > span').length).toBe(3);
    ui.unmount();
  });

  it('takes the class it is given beside its own', () => {
    const ui = mount(<Diff lines={lineDiff('a', 'b')} className="save-preview" />);
    expect(ui.get('pre')?.className).toBe('diff save-preview');
    ui.unmount();
  });
});
