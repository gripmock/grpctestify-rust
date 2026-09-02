import { afterEach, describe, expect, it, vi } from 'vitest';
import { act } from 'react';
import { ImportPanel } from './ImportPanel';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';

const panel = <ToastProvider><ImportPanel /></ToastProvider>;
const settle = () => new Promise(r => setTimeout(r, 20));

describe('a grpcurl line the workbench could not read', () => {
  afterEach(() => { vi.unstubAllGlobals(); });

  it('says so in the workbench’s words, or with the status it gave', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => ({ ok: false, status: 502, statusText: 'Bad Gateway', json: async () => { throw new Error('no body'); } })));
    const ui = mount(panel);
    ui.type('textarea', 'grpcurl -plaintext localhost:4770 a.A/One');
    ui.click(ui.all('button').find(b => b.textContent?.includes('mport'))!);
    await settle();
    expect(ui.get('.assert.is-fail').textContent).toContain('The workbench could not read that command (502 Bad Gateway)');
    ui.unmount();
  });
});

describe('the command a request for an import brings along', () => {
  it('fills the box, and fills it again for the next request', () => {
    useStore.setState({ importIntent: 0, importPrefill: null });
    const ui = mount(panel);
    expect((ui.get('textarea') as HTMLTextAreaElement).value).toBe('');
    ui.type('textarea', 'curl https://x');
    act(() => { useStore.getState().requestImport('grpcurl -plaintext localhost:4770 a.A/One'); });
    expect((ui.get('textarea') as HTMLTextAreaElement).value).toBe('grpcurl -plaintext localhost:4770 a.A/One');
    ui.unmount();
  });
});
