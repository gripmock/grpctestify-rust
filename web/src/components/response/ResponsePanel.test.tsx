import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ResponsePanel } from './ResponsePanel';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';
import type { CallResult } from '../../lib/types';

vi.mock('../MonacoEditor', () => ({ MonacoEditor: () => null }));

const answered: CallResult = {
  status: 'ok', statusCode: 0, messages: [{ ok: true }], headers: { 'content-type': 'application/grpc' },
  trailers: {}, error: null, durationMs: 4,
};

const panel = (
  <ModalProvider>
    <ToastProvider>
      <ResponsePanel />
    </ToastProvider>
  </ModalProvider>
);

const clipboard = Object.getOwnPropertyDescriptor(Navigator.prototype, 'clipboard');

describe('copying from the response over plain http', () => {
  beforeEach(() => {
    useStore.setState({ response: answered, responseTab: 'headers', workspacePath: null } as never);
    Object.defineProperty(navigator, 'clipboard', { value: undefined, configurable: true });
    document.execCommand = vi.fn(() => true);
  });

  afterEach(() => {
    if (clipboard) Object.defineProperty(Navigator.prototype, 'clipboard', clipboard);
    else delete (navigator as { clipboard?: unknown }).clipboard;
  });

  it('still copies a header when the browser offers no clipboard api', async () => {
    const ui = mount(panel);
    ui.click('.meta-value');
    await new Promise(r => setTimeout(r, 0));
    expect(document.execCommand).toHaveBeenCalledWith('copy');
    expect(document.body.textContent).toContain('content-type copied');
    ui.unmount();
  });
});

describe('the answer and the tab that names it', () => {
  it('marks the pane as the panel of the tab that is on', () => {
    useStore.setState({ response: answered, responseTab: 'headers', workspacePath: null } as never);
    const ui = mount(panel);
    const on = ui.all('[role="tab"]').find(t => t.getAttribute('aria-selected') === 'true')!;
    const pane = ui.get('[role="tabpanel"]');
    expect(pane.id).toBe(on.getAttribute('aria-controls'));
    expect(pane.getAttribute('aria-labelledby')).toBe(on.id);
    ui.unmount();
  });

  it('opens each header into a menu the keyboard can walk', () => {
    useStore.setState({ response: answered, responseTab: 'headers', workspacePath: null } as never);
    const ui = mount(panel);
    ui.click('[aria-label="Assert content-type"]');
    const menu = ui.get('[role="menu"]');
    expect(menu.getAttribute('aria-label')).toBe('Assert content-type');
    expect(document.activeElement?.getAttribute('role')).toBe('menuitem');
    ui.unmount();
  });
});
