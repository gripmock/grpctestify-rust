import { describe, expect, it, vi, beforeEach } from 'vitest';
import { ExtractsView } from './SectionViews';
import { useStore } from '../../lib/store';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { mount } from 'luvo/test/render';

const answered = () => {
  useStore.setState({
    activeStep: 0,
    documents: [],
    response: {
      status: 'ok', statusCode: 200, messages: [{ id: 7 }], headers: {}, trailers: {},
      error: null, durationMs: 3, fromStep: 0,
    } as never,
  });
};

describe('what an extraction takes', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn(async () => new Response(
      JSON.stringify({ outputs: ['7'], error: null }),
      { headers: { 'content-type': 'application/json' } },
    )));
  });

  it('is read from the answer without asking', async () => {
    answered();
    const ui = mount(<ToastProvider><ModalProvider><ExtractsView extracts={{ user: '.id' }} /></ModalProvider></ToastProvider>);
    await new Promise(r => setTimeout(r, 0));
    expect(fetch).toHaveBeenCalled();
    const body = JSON.parse((fetch as never as { mock: { calls: [string, { body: string }][] } }).mock.calls[0][1].body);
    expect(body.expr).toBe('.id');
    ui.unmount();
  });

  it('asks nothing when there is no answer to read', async () => {
    useStore.setState({ activeStep: 0, response: null });
    const ui = mount(<ToastProvider><ModalProvider><ExtractsView extracts={{ user: '.id' }} /></ModalProvider></ToastProvider>);
    await new Promise(r => setTimeout(r, 0));
    expect(fetch).not.toHaveBeenCalled();
    ui.unmount();
  });
});

describe('renaming an extraction that later steps read', () => {
  it('renames the file and everything in it that reads the name', async () => {
    const calls: { url: string; body: unknown }[] = [];
    vi.stubGlobal('fetch', vi.fn(async (url: string, init?: { body?: string }) => {
      calls.push({ url, body: init?.body ? JSON.parse(init.body) : null });
      if (String(url).includes('/api/rename-variable')) {
        return { ok: true, status: 200, json: async () => ({ rewritten: 3 }) } as never;
      }
      return { ok: false, status: 404, json: async () => ({}), text: async () => '' } as never;
    }));
    useStore.setState({
      activeStep: 0,
      response: null,
      workspacePath: 'chain.httf',
      rawContent: null, rawOriginal: null,
      workspaceOriginal: null,
      request: { endpoint: 'GET /a', headers: {}, bodies: [] },
      documents: [
        { endpoint: 'GET /a', produces: ['user'], consumes: [] },
        { endpoint: 'GET /b/{{user}}', produces: [], consumes: ['user'] },
      ] as never,
    });
    const ui = mount(<ToastProvider><ModalProvider><ExtractsView extracts={{ user: '.id' }} /></ModalProvider></ToastProvider>);

    const buttons = () => [...ui.container.querySelectorAll('button')];
    buttons().find(b => /edit/i.test(b.title || b.getAttribute('aria-label') || b.textContent || ''))
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await new Promise(r => setTimeout(r, 0));

    const name = ui.container.querySelector('input') as HTMLInputElement;
    const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!;
    setter.call(name, 'account');
    name.dispatchEvent(new Event('input', { bubbles: true }));
    await new Promise(r => setTimeout(r, 0));

    buttons().find(b => /done/i.test(b.textContent ?? ''))
      ?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    await new Promise(r => setTimeout(r, 10));

    const asked = calls.find(c => c.url.includes('/api/rename-variable'));
    expect(asked?.body).toEqual({ path: 'chain.httf', from: 'user', to: 'account', dataset: false });
    ui.unmount();
    vi.unstubAllGlobals();
  });
});
