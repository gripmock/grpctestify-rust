import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act } from 'react';
import { Sidebar } from './Sidebar';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';

const rail = (
  <ModalProvider>
    <ToastProvider>
      <Sidebar />
    </ToastProvider>
  </ModalProvider>
);

const settle = () => new Promise(r => setTimeout(r, 20));

describe('making a new file when the workbench refuses', () => {
  beforeEach(() => {
    useStore.setState({ collections: [], collectionsRead: 'ok', refreshCollections: () => {} } as never);
  });
  afterEach(() => { vi.unstubAllGlobals(); });

  async function ask(ui: ReturnType<typeof mount>) {
    ui.click('.empty-state .btn');
    await settle();
    ui.type('dialog input', 'probe');
    ui.click('dialog .btn.is-primary');
    await settle();
  }

  it('says what the workbench said', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => ({
      ok: false, status: 403, statusText: 'Forbidden', text: async () => 'the directory is read-only',
    })));
    const ui = mount(rail);
    await ask(ui);
    expect(document.body.textContent).toContain('the directory is read-only');
    expect(document.body.textContent).not.toContain('Failed');
    ui.unmount();
  });

  it('names the file when the workbench said nothing', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: false, status: 500, statusText: 'x', text: async () => '' })));
    const ui = mount(rail);
    await ask(ui);
    expect(document.body.textContent).toContain('The workbench could not write probe.gctf');
    ui.unmount();
  });

  it('says the workbench could not be reached when nothing answered', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('Failed to fetch'); }));
    const ui = mount(rail);
    await ask(ui);
    expect(document.body.textContent).toContain('The workbench could not be reached — nothing was written');
    ui.unmount();
  });
});

describe('making a folder the workbench will not keep', () => {
  beforeEach(() => {
    useStore.setState({
      collections: [{ path: 'api', is_dir: true } as never], collectionsRead: 'ok', refreshCollections: () => {},
    } as never);
  });
  afterEach(() => { vi.unstubAllGlobals(); });

  const REFUSED = '.staging is a hidden path — the workbench would never list it again, and could not remove it';

  async function askFolder(ui: ReturnType<typeof mount>, name: string) {
    ui.type('dialog input', name);
    ui.click('dialog .btn.is-primary');
    await settle();
  }

  function openNewFolder(ui: ReturnType<typeof mount>) {
    const row = ui.get('.row.tree-node');
    act(() => { row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true })); });
    const item = ui.all('.menu-item').find(m => m.textContent?.includes('New folder'));
    ui.click(item!);
  }

  it('says what the workbench said, and asks again rather than losing the name', async () => {
    const fetchMock = vi.fn(async () => ({ ok: false, status: 400, statusText: 'Bad Request', text: async () => REFUSED }));
    vi.stubGlobal('fetch', fetchMock as never);
    const ui = mount(rail);
    await settle();
    openNewFolder(ui);
    await settle();
    await askFolder(ui, '.staging');

    expect(document.body.textContent).toContain('hidden path');
    expect(document.querySelector('dialog input')).toBeTruthy();
    expect((document.querySelector('dialog input') as HTMLInputElement).value).toBe('.staging');
    ui.click('dialog .btn.is-quiet');
    await settle();
    ui.unmount();
  });

  it('says the workbench could not be reached when nothing answered', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('Failed to fetch'); }));
    const ui = mount(rail);
    await settle();
    openNewFolder(ui);
    await settle();
    await askFolder(ui, 'notes');
    expect(document.body.textContent).toContain('The workbench could not be reached — nothing was created');
    ui.click('dialog .btn.is-quiet');
    await settle();
    ui.unmount();
  });
});
