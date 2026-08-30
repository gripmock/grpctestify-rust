import { describe, expect, it, vi } from 'vitest';
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
