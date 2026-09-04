import { describe, expect, it, vi } from 'vitest';
import { act } from 'react';
import { useStore } from '../../lib/store';
import { CommandPalette } from './CommandPalette';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { mount } from 'luvo/test/render';
import type { CommandUi } from '../../lib/commands';

const ui: CommandUi = {
  openPalette: () => {}, closePalette: () => {}, openHelp: () => {},
  saveFile: () => {}, openImport: () => {}, say: () => {},
};

describe('a dialog closed by something other than its own buttons', () => {
  it('tells the shell it closed', () => {
    const onClose = vi.fn();
    const view = mount(
      <ToastProvider>
        <CommandPalette open onClose={onClose} ui={ui} />
      </ToastProvider>,
    );

    const dialog = view.get('dialog') as HTMLDialogElement;
    dialog.close();
    expect(onClose).toHaveBeenCalled();
    view.unmount();
  });
});

describe('what the palette lists', () => {
  it('follows the files the rail learns about while it is open', () => {
    useStore.setState({ collections: [{ path: 'a.gctf', name: 'a.gctf', is_dir: false, tags: [] }] });
    const view = mount(
      <ToastProvider>
        <CommandPalette open onClose={() => {}} ui={ui} />
      </ToastProvider>,
    );
    const paths = () => view.all('[role="option"] .mono').map(el => el.textContent);
    expect(paths()).toEqual(['a.gctf']);

    act(() => {
      useStore.setState({ collections: [
        { path: 'a.gctf', name: 'a.gctf', is_dir: false, tags: [] },
        { path: 'b.httf', name: 'b.httf', is_dir: false, tags: [] },
      ] });
    });
    expect(paths()).toEqual(['a.gctf', 'b.httf']);
    view.unmount();
  });

  it('is a listbox whose highlighted option the input names', () => {
    useStore.setState({ collections: [{ path: 'a.gctf', name: 'a.gctf', is_dir: false, tags: [] }] });
    const view = mount(
      <ToastProvider>
        <CommandPalette open onClose={() => {}} ui={ui} />
      </ToastProvider>,
    );
    const options = view.all('[role="listbox"] [role="option"]');
    expect(options.length).toBeGreaterThan(1);
    expect(options[0].getAttribute('aria-selected')).toBe('true');
    expect(view.get('input').getAttribute('aria-activedescendant')).toBe(options[0].id);

    view.key('input', 'ArrowDown');
    const after = view.all('[role="listbox"] [role="option"]');
    expect(after[1].getAttribute('aria-selected')).toBe('true');
    expect(view.get('input').getAttribute('aria-activedescendant')).toBe(after[1].id);
    view.unmount();
  });
});
