import { describe, expect, it, vi } from 'vitest';
import { KeyboardShortcutHelp } from './KeyboardShortcutHelp';
import { mount } from 'luvo/test/render';

describe('the shortcuts panel', () => {
  it('is a modal dialog the browser owns', () => {
    const ui = mount(<KeyboardShortcutHelp open onClose={() => {}} />);
    const dialog = ui.get('dialog') as HTMLDialogElement;
    expect(dialog.open).toBe(true);
    expect(dialog.getAttribute('aria-label')).toBe('Keyboard shortcuts');
    ui.unmount();
  });

  it('stays shut until it is asked for', () => {
    const ui = mount(<KeyboardShortcutHelp open={false} onClose={() => {}} />);
    expect((ui.get('dialog') as HTMLDialogElement).open).toBe(false);
    ui.unmount();
  });

  it('draws every group into one list', () => {
    const ui = mount(<KeyboardShortcutHelp open onClose={() => {}} />);
    expect(ui.all('.keys-list').length).toBeGreaterThanOrEqual(1);
    expect(ui.all('.keys-row').length).toBeGreaterThan(10);
    expect(ui.all('.keys-group-name').length).toBeGreaterThan(3);
    ui.unmount();
  });

  it('finds a chord by the words for its keys', () => {
    const ui = mount(<KeyboardShortcutHelp open onClose={() => {}} />);
    ui.type('.keys-search input', 'shift');
    const rows = ui.all('.keys-row').map(r => r.textContent ?? '');
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.every(r => /⇧|shift/i.test(r))).toBe(true);
    ui.unmount();
  });

  it('says how much the filter left', () => {
    const ui = mount(<KeyboardShortcutHelp open onClose={() => {}} />);
    expect(ui.all('.keys-count')).toHaveLength(0);
    ui.type('.keys-search input', 'shift');
    expect(ui.get('.keys-count').textContent).toMatch(/^\d+ of \d+$/);
    ui.unmount();
  });

  it('says so when nothing matches', () => {
    const ui = mount(<KeyboardShortcutHelp open onClose={() => {}} />);
    ui.type('.keys-search input', 'zzzzz');
    expect(ui.all('.keys-row')).toHaveLength(0);
    expect(ui.get('.empty-state').textContent).toContain('zzzzz');
    ui.unmount();
  });

  it('forgets the filter on the way out', () => {
    const onClose = vi.fn();
    const ui = mount(<KeyboardShortcutHelp open onClose={onClose} />);
    ui.type('.keys-search input', 'tab');
    ui.click('[aria-label="Close"]');
    expect(onClose).toHaveBeenCalled();
    ui.update(<KeyboardShortcutHelp open onClose={onClose} />);
    expect((ui.get('.keys-search input') as HTMLInputElement).value).toBe('');
    ui.unmount();
  });

  it('keeps the keys of a control in the panel about keys', () => {
    const ui = mount(<KeyboardShortcutHelp open onClose={() => {}} />);
    expect(ui.get('.keys-local').textContent).toContain('the file tree');
    ui.unmount();
  });
});
