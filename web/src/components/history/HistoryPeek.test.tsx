import { createRef } from 'react';
import { describe, expect, it } from 'vitest';
import { HistoryPeek } from './HistoryPeek';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { mount } from 'luvo/test/render';
import type { HistoryEntry } from '../../lib/types';

const entry = (over: Partial<HistoryEntry> = {}): HistoryEntry => ({
  id: 'h1',
  timestamp: 1,
  endpoint: 'pkg.Svc/M',
  bodies: ['{"name": "{{who}}"}'],
  headers: {},
  response: {
    status: 'ok', statusCode: 0, messages: [{ ok: true }], headers: {}, trailers: {},
    error: null, durationMs: 7,
  },
  ...over,
});

const peek = (e: HistoryEntry) => (
  <ToastProvider>
    <HistoryPeek
      entry={e}
      top={0}
      panelRef={createRef<HTMLDivElement>()}
      onClose={() => {}}
      onOpen={() => {}}
      onReplay={() => {}}
    />
  </ToastProvider>
);

describe('what the card says about the request', () => {
  it('names how many went out as something else', () => {
    const ui = mount(peek(entry({ resolved: ['who'] })));
    const said = ui.get('.peek-filled');
    expect(said.textContent).toBe('1 name filled in');
    expect(said.title).toContain('who');
    ui.unmount();
  });

  it('says nothing when the wire carried what the line shows', () => {
    const ui = mount(peek(entry()));
    expect(ui.all('.peek-filled')).toEqual([]);
    ui.unmount();
  });

  it('says which row a call on a file of cases was', () => {
    const ui = mount(peek(entry({ datasetRow: 1 })));
    expect(ui.container.textContent).toContain('row 2');
    ui.unmount();
  });
});

describe('what the card says about the headers', () => {
  it('names them, but keeps a credential to itself', () => {
    const ui = mount(peek(entry({
      headers: { authorization: 'Bearer abc123', 'x-request-id': 'r-1', 'x-api-key': '{{API_KEY}}' },
    })));
    const said = ui.get('.peek-headers').title;
    expect(said).toContain('authorization: ••••••');
    expect(said).toContain('x-request-id: r-1');
    expect(said).toContain('x-api-key: {{API_KEY}}');
    expect(said).not.toContain('abc123');
    ui.unmount();
  });
});
