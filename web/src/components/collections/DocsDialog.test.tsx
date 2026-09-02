import { describe, expect, it, vi, afterEach } from 'vitest';
import { act } from 'react';
import { DocsDialog } from './DocsDialog';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { mount } from 'luvo/test/render';

const PAGES = [
  { name: 'index.md', markdown: '# API Documentation\n\n| Service | Methods |\n|---|---|\n| [/v1](v1.md) | 1 |\n' },
  {
    name: 'v1.md',
    markdown: '# /v1\n\n## users\n\n**Endpoint:** `GET /v1/users`\n\nCall it with `curl`:\n\n```sh\ncurl -L \'https://api.test/v1/users\'\n```\n\nAsserts:\n\n- `@status() == 200`\n- `.id != ""`\n',
  },
  { name: 'pkg.Svc.md', markdown: '# pkg.Svc\n\n## greeting\n\n**Endpoint:** `pkg.Svc/SayHello`\n' },
];

function serve(pages: unknown) {
  vi.stubGlobal('fetch', vi.fn(async () => ({ ok: true, json: async () => pages })));
}

const dialog = (paths: string[]) => <ToastProvider><DocsDialog paths={paths} onClose={() => {}} /></ToastProvider>;

async function open() {
  const ui = mount(dialog(['a.gctf']));
  await act(async () => { await Promise.resolve(); });
  return ui;
}

async function settle() {
  await act(async () => { await Promise.resolve(); await Promise.resolve(); });
}

afterEach(() => { vi.unstubAllGlobals(); });

describe('the pages a suite would be documented as', () => {
  it('lists them by the title each page carries, not by its file name', async () => {
    serve(PAGES);
    const ui = await open();
    expect(ui.all('.docs-pages .row').map(r => r.textContent)).toEqual(['overview', '/v1', 'pkg.Svc']);
    ui.unmount();
  });

  it('draws a list as a list, one item per line', async () => {
    serve(PAGES);
    const ui = await open();
    ui.click(ui.all('.docs-pages .row')[1]!);
    expect(ui.all('.docs-list li').map(li => li.textContent)).toEqual(['@status() == 200', '.id != ""']);
    ui.unmount();
  });

  it('offers a copy per block, named for what the block holds', async () => {
    serve(PAGES);
    const ui = await open();
    ui.click(ui.all('.docs-pages .row')[1]!);
    expect(ui.all('.docs-copy').map(b => b.getAttribute('aria-label'))).toEqual(['Copy this command']);
    ui.unmount();
  });

  it('narrows the list to what a query names, and keeps the way back', async () => {
    serve(PAGES);
    const ui = await open();
    ui.type('.docs-filter', 'SayHello');
    expect(ui.all('.docs-pages .row').map(r => r.textContent)).toEqual(['overview', 'pkg.Svc']);
    ui.type('.docs-filter', 'nothing-here');
    expect(ui.all('.docs-pages .row').map(r => r.textContent)).toEqual(['overview']);
    expect(ui.get('.docs-none').textContent).toContain('nothing-here');
    ui.unmount();
  });

  it('opens the page a link names', async () => {
    serve(PAGES);
    const ui = await open();
    ui.click('.docs-link.is-page');
    expect(ui.get('.docs-page .field-label').textContent).toBe('/v1');
    ui.unmount();
  });

  it('says a set with nothing to document has nothing, rather than failing', async () => {
    serve([]);
    const ui = await open();
    expect(ui.get('.empty-state').textContent).toContain('nothing to document');
    ui.unmount();
  });
});

describe('a fetch that fails and then one that does not', () => {
  it('shows only the pages once they arrive', async () => {
    const fetchMock = vi.fn()
      .mockImplementationOnce(async () => { throw new Error('the workbench went away'); })
      .mockImplementation(async () => ({ ok: true, json: async () => PAGES }));
    vi.stubGlobal('fetch', fetchMock);
    const ui = await open();
    await settle();
    expect(ui.get('.assert.is-fail').textContent).toContain('the workbench went away');

    ui.update(dialog(['b.gctf']));
    await settle();
    expect(ui.all('.assert.is-fail')).toEqual([]);
    expect(ui.all('.docs-pages .row').map(r => r.textContent)).toEqual(['overview', '/v1', 'pkg.Svc']);
    ui.unmount();
  });
});
