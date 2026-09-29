import { afterEach, describe, expect, it, vi } from 'vitest';
import { act } from 'react';
import { PlayLayout } from './PlayLayout';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';
import { SYNC_EVERY } from '../../lib/poll-tick';

const layout = () => (
  <ToastProvider>
    <ModalProvider>
      <PlayLayout />
    </ModalProvider>
  </ToastProvider>
);

describe('a run the workbench had no room for', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    useStore.setState({ runRefused: null } as never);
  });

  it('is said once, as a refusal rather than a failed run', () => {
    vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
      const at = String(url);
      if (at.includes('/api/collections') || at.includes('/api/jobs')) return new Response('[]', { status: 200 });
      return new Response('{}', { status: 200 });
    }));
    vi.stubGlobal('EventSource', class { close() {} addEventListener() {} } as never);
    const ui = mount(layout());
    act(() => {
      useStore.setState({ runRefused: { text: 'four runs are already going — this one was not started', nonce: 1 } } as never);
    });
    const toasts = ui.all('.toast');
    expect(toasts).toHaveLength(1);
    expect(toasts[0].textContent).toContain('four runs are already going');
    expect(toasts[0].getAttribute('role')).toBe('alert');
    ui.unmount();
  });
});

describe('a file changed on disk while the counter stands still', () => {
  afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });

  it('is read again by the fallback sync, not only when the counter moves', async () => {
    vi.useFakeTimers();
    const asked: string[] = [];
    vi.stubGlobal('fetch', vi.fn(async (url: unknown) => {
      const at = String(url);
      asked.push(at);
      if (at.includes('/api/info')) return new Response(JSON.stringify({ collections_mtime: 7, status: 'ok' }), { status: 200 });
      if (at.includes('/api/collections') || at.includes('/api/jobs')) return new Response('[]', { status: 200 });
      return new Response('{}', { status: 200 });
    }));
    vi.stubGlobal('EventSource', class { close() {} addEventListener() {} } as never);
    let synced = 0;
    useStore.setState({ collectionsMtime: 7, syncOpenFiles: async () => { synced += 1; return []; } } as never);

    const ui = mount(layout());
    await act(async () => { await vi.advanceTimersByTimeAsync(3000 * (SYNC_EVERY - 1)); });
    expect(synced).toBe(0);

    await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
    expect(synced).toBe(1);
    ui.unmount();
  });
});
