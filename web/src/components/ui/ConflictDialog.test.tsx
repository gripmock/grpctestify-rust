import { describe, expect, it, beforeEach } from 'vitest';
import { ConflictDialog } from './ConflictDialog';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';

describe('answering a file that changed on disk', () => {
  const choices: string[] = [];

  beforeEach(() => {
    choices.length = 0;
    useStore.setState({
      saveConflict: { path: 'auth/login.gctf', mine: 'mine\n', theirs: 'theirs\n', raw: false },
      resolveSaveConflict: (async (choice: string) => { choices.push(choice); }) as never,
    } as never);
  });

  const open = () => mount(<ToastProvider><ConflictDialog /></ToastProvider>);

  it('names the file that changed', () => {
    const ui = open();
    expect(ui.byText('auth/login.gctf')).toHaveLength(1);
    ui.unmount();
  });

  it('takes the disk version when that is what was pressed', () => {
    const ui = open();
    ui.click([...ui.container.querySelectorAll('button')].find(b => /take disk version/i.test(b.textContent ?? ''))!);
    expect(choices).toEqual(['reload']);
    ui.unmount();
  });

  it('overwrites only when overwrite was pressed', () => {
    const ui = open();
    ui.click([...ui.container.querySelectorAll('button')].find(b => /overwrite with mine/i.test(b.textContent ?? ''))!);
    expect(choices).toEqual(['overwrite']);
    ui.unmount();
  });

  it('cancels without writing anything', () => {
    const ui = open();
    ui.click([...ui.container.querySelectorAll('button')].find(b => /^cancel$/i.test((b.textContent ?? '').trim()))!);
    expect(choices).toEqual(['cancel']);
    ui.unmount();
  });

  it('is not there when no save was refused', () => {
    useStore.setState({ saveConflict: null } as never);
    const ui = open();
    expect(ui.container.querySelector('dialog')?.open ?? false).toBe(false);
    ui.unmount();
  });

  it('draws the overwrite as the destructive choice it is', () => {
    const ui = open();
    const overwrite = [...ui.container.querySelectorAll('button')]
      .find(b => /overwrite with mine/i.test(b.textContent ?? ''))!;
    expect(overwrite.className).toContain('is-danger');
    ui.unmount();
  });
});
