import { describe, expect, it } from 'vitest';
import { HistoryPanel } from './HistoryPanel';
import { useStore } from '../../lib/store';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { mount } from 'luvo/test/render';
import type { HistoryEntry } from '../../lib/types';

const panel = () => (
  <ToastProvider>
    <ModalProvider>
      <HistoryPanel />
    </ModalProvider>
  </ToastProvider>
);

const runEntry: HistoryEntry = {
  id: 'h1', timestamp: 1, endpoint: 'api/probe.httf', bodies: [], headers: {},
  collectionPath: 'api/probe.httf',
  kind: 'run',
  response: { status: 'ok', statusCode: null, messages: [{}], headers: {}, trailers: {}, error: null, durationMs: 3 },
};

describe('replaying a line a run wrote', () => {
  it('waits for the file to open', async () => {
    let opened: ((ok: boolean) => void) | null = null;
    const ran: string[] = [];
    useStore.setState({
      history: [runEntry],
      loadCollection: (path: string) => { ran.push(`open:${path}`); return new Promise<boolean>(r => { opened = r; }); },
      runTest: async () => { ran.push('run'); },
    } as never);

    const ui = mount(panel());
    ui.click('.history-acts button');
    expect(ran).toEqual(['open:api/probe.httf']);

    opened!(true);
    await Promise.resolve();
    expect(ran).toEqual(['open:api/probe.httf', 'run']);
    ui.unmount();
  });

  it('says so, and runs nothing, when the file is gone', async () => {
    const ran: string[] = [];
    useStore.setState({
      history: [runEntry],
      loadCollection: async () => false,
      runTest: async () => { ran.push('run'); },
    } as never);

    const ui = mount(panel());
    ui.click('.history-acts button');
    await new Promise(r => setTimeout(r, 30));
    expect(ran).toEqual([]);
    expect(ui.container.ownerDocument.body.textContent).toContain('api/probe.httf is not in this workbench');
    ui.unmount();
  });
});

describe('what the card says about the connection', () => {
  const withConn = (connection: HistoryEntry['connection']): HistoryEntry => ({
    ...runEntry, id: 'peek', kind: undefined, connection,
  });

  const open = (entry: HistoryEntry) => {
    useStore.setState({ history: [entry] } as never);
    const ui = mount(panel());
    ui.click('.history-row');
    return ui;
  };

  it('says only what was kept', () => {
    const ui = open(withConn({ address: 'http://127.0.0.1:8899' } as never));
    const line = ui.get('.peek-conn').textContent ?? '';
    expect(line).toContain('http://127.0.0.1:8899');
    expect(line).not.toContain('undefined');
    ui.unmount();
  });

  it('says the transport when the entry kept one', () => {
    const ui = open(withConn({ address: '127.0.0.1:4790', protocol: 'grpc', tls: false } as never));
    expect(ui.get('.peek-conn').textContent).toContain('127.0.0.1:4790 · grpc');
    ui.unmount();
  });

  it('says tls when it was one', () => {
    const ui = open(withConn({ address: 'api.example.com:443', protocol: 'grpc', tls: true } as never));
    expect(ui.get('.peek-conn').textContent).toContain('api.example.com:443 · grpc · tls');
    ui.unmount();
  });

  it('says when there is nothing to say', () => {
    const ui = open(withConn(undefined));
    expect(ui.get('.peek-conn').textContent).toContain('recorded before the connection was kept');
    ui.unmount();
  });
});

describe('the shape a line says a call had', () => {
  const call = (over: Partial<HistoryEntry> = {}): HistoryEntry => ({
    id: 'c1', timestamp: 2, endpoint: 'feed.Feed/Subscribe', bodies: ['{}'], headers: {},
    response: { status: 'ok', statusCode: 0, messages: [{}], headers: {}, trailers: {}, error: null, durationMs: 5, shape: 'server' },
    ...over,
  });

  it('wears the badge the rest of the workbench wears', () => {
    useStore.setState({ history: [call()], sidebarTab: 'history' });
    const ui = mount(panel());
    const badge = ui.all('.history-payload .badge.is-kind')[0];
    expect(badge?.textContent).toBe('server');
    expect(badge?.className).toContain('kind-down');
    ui.unmount();
  });

  it('says nothing about a call whose shape was never resolved', () => {
    useStore.setState({ history: [call({ response: { ...call().response, shape: null } })], sidebarTab: 'history' });
    const ui = mount(panel());
    expect(ui.all('.history-payload .badge.is-kind')).toEqual([]);
    ui.unmount();
  });

  it('says nothing about an HTTP call or a run', () => {
    useStore.setState({
      history: [call({ endpoint: 'GET /v1/users' }), { ...runEntry, id: 'r2' }],
      sidebarTab: 'history',
    });
    const ui = mount(panel());
    expect(ui.all('.history-payload .badge.is-kind')).toEqual([]);
    ui.unmount();
  });
});
