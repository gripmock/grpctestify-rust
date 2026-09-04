import { describe, expect, it, beforeEach, afterEach, vi } from 'vitest';
import { act } from 'react';
import { TabBar } from './TabBar';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';
import type { Tab } from '../../lib/types';

function tab(id: string, path: string | null): Tab {
  return {
    id, label: path ?? 'Untitled', endpoint: 'a.A/One', headers: {}, bodies: ['{}'],
    response: null, requestTab: 'body', gctfTab: 'request', responseTab: 'response',
    collectionPath: path, collectionParsed: null, collectionOriginal: null,
    rawContent: null, rawOriginal: null,
  };
}

const strip = (
  <ModalProvider>
    <ToastProvider>
      <TabBar />
    </ToastProvider>
  </ModalProvider>
);

describe('closing the tabs whose file is gone', () => {
  beforeEach(() => {
    useStore.setState({
      tabs: [tab('a', 'here.gctf'), tab('b', 'deleted.gctf'), tab('c', null)],
      activeTabId: 'a',
      collections: [{ path: 'here.gctf', name: 'here.gctf', is_dir: false, tags: [] }],
      collectionsRead: 'ok',
    });
  });

  it('counts the ones the collections no longer list', () => {
    const ui = mount(strip);
    ui.click('[aria-label^="Actions for"]');
    const item = ui.all('.menu-item').find(b => b.textContent?.includes('not on disk'));
    expect(item?.textContent).toContain('Close 1 not on disk');
    expect(item?.getAttribute('title')).toBe('deleted.gctf');
    ui.unmount();
  });

  it('closes them and leaves the ones that still have a file', () => {
    const ui = mount(strip);
    ui.click('[aria-label^="Actions for"]');
    const item = ui.all('.menu-item').find(b => b.textContent?.includes('not on disk'))!;
    ui.click(item);
    expect(useStore.getState().tabs.map(t => t.collectionPath)).toEqual(['here.gctf', null]);
    ui.unmount();
  });
});

describe('closing every tab', () => {
  beforeEach(() => {
    useStore.setState({
      tabs: [tab('a', 'here.gctf'), tab('b', null), tab('c', null)],
      activeTabId: 'a',
      request: { endpoint: 'a.A/One', headers: {}, bodies: ['{}'] },
      collections: [{ path: 'here.gctf', name: 'here.gctf', is_dir: false, tags: [] }],
      collectionsRead: 'ok',
    });
  });

  it('is offered where the tabs are listed', () => {
    const ui = mount(strip);
    ui.click('[title="All 3 open tabs"]');
    const item = ui.all('.menu-item').find(b => b.textContent?.includes('Close all'));
    expect(item?.textContent).toContain('Close all 3 tabs');
    ui.unmount();
  });
});

describe('what a share hands over', () => {
  beforeEach(() => {
    useStore.setState({
      tabs: [tab('a', null)],
      activeTabId: 'a',
      request: { endpoint: 'a.A/One', headers: { 'x-run': '{{run}}' }, bodies: ['{"who": "{{who}}"}'] },
      address: '127.0.0.1:4790',
      addressTouched: true,
      workspacePath: null,
      collectionParsed: null,
      collections: [],
      collectionsRead: 'ok',
      share: null,
    } as never);
  });

  const openShare = (ui: ReturnType<typeof mount>) => {
    ui.click('[aria-label^="Actions for"]');
    const item = ui.all('.menu-item').find(b => b.textContent?.includes('Share'))!;
    ui.click(item);
  };

  it('opens the dialog for a request with no file behind it', () => {
    const ui = mount(strip);
    openShare(ui);
    expect(ui.get('dialog[aria-label="Share request"]').hasAttribute('open')).toBe(true);
    ui.unmount();
  });

  it('names the variables it cannot answer for', () => {
    const ui = mount(strip);
    openShare(ui);
    const text = ui.get('dialog[aria-label="Share request"]').textContent ?? '';
    expect(text).toContain('{{who}}');
    expect(text).toContain('{{run}}');
    expect(text).toContain('travel as written');
    ui.unmount();
  });

  it('says where the link opens', () => {
    const ui = mount(strip);
    openShare(ui);
    expect(ui.get('dialog[aria-label="Share request"]').textContent).toContain('127.0.0.1:4790');
    ui.unmount();
  });

  it('offers a link, not a dialog, for a file', () => {
    useStore.setState({
      tabs: [tab('a', 'here.gctf')], activeTabId: 'a', workspacePath: 'here.gctf', share: null,
    } as never);
    const ui = mount(strip);
    ui.click('[aria-label^="Actions for"]');
    const item = ui.all('.menu-item').find(b => b.textContent?.includes('link to this file'));
    expect(item, 'the item names what it does').toBeTruthy();
    ui.click(item!);
    expect(ui.get('dialog[aria-label="Share request"]').hasAttribute('open')).toBe(false);
    ui.unmount();
  });
});

describe('the control that says where the response sits', () => {
  const withWidth = (matches: boolean) => {
    const original = window.matchMedia;
    window.matchMedia = ((query: string) => ({
      matches, media: query, onchange: null,
      addEventListener() {}, removeEventListener() {},
      addListener() {}, removeListener() {}, dispatchEvent: () => false,
    })) as unknown as typeof window.matchMedia;
    return () => { window.matchMedia = original; };
  };

  it('offers both shapes when the window can hold them', () => {
    const restore = withWidth(true);
    useStore.setState({ tabs: [tab('1', 'a.gctf')], activeTabId: '1' } as never);
    const ui = mount(<ToastProvider><ModalProvider><TabBar /></ModalProvider></ToastProvider>);
    const side = ui.all('.layout-pick [role="radio"]')[0]!;
    expect(side.hasAttribute('disabled')).toBe(false);
    ui.unmount();
    restore();
  });

  it('refuses the side-by-side shape on a window too narrow for it, and says why', () => {
    const restore = withWidth(false);
    useStore.setState({ tabs: [tab('1', 'a.gctf')], activeTabId: '1' } as never);
    const ui = mount(<ToastProvider><ModalProvider><TabBar /></ModalProvider></ToastProvider>);
    const side = ui.all('.layout-pick [role="radio"]')[0]!;
    expect(side.hasAttribute('disabled')).toBe(true);
    expect(side.title).toContain('too narrow');
    ui.unmount();
    restore();
  });
});

describe('making a link to a request that is not a file', () => {
  beforeEach(() => {
    useStore.setState({
      tabs: [tab('a', null)],
      activeTabId: 'a',
      request: { endpoint: 'a.A/One', headers: {}, bodies: ['{}'] },
      share: { headers: {}, ttl: 7 },
      tls: false,
      tlsInsecure: false,
    });
  });
  afterEach(() => { vi.unstubAllGlobals(); });

  const settle = () => new Promise(r => setTimeout(r, 20));

  it('says what the workbench said when it refused', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => ({
      ok: false, status: 403, statusText: 'Forbidden', text: async () => 'links are off in this workbench',
    })));
    const ui = mount(strip);
    ui.click('[aria-label="Share request"] .btn.is-primary');
    await settle();
    expect(document.body.textContent).toContain('links are off in this workbench');
    expect(document.body.textContent).not.toContain('Failed');
    ui.unmount();
  });

  it('says the workbench could not be reached when nothing answered', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => { throw new TypeError('Failed to fetch'); }));
    const ui = mount(strip);
    ui.click('[aria-label="Share request"] .btn.is-primary');
    await settle();
    expect(document.body.textContent).toContain('The workbench could not be reached — nothing was shared');
    ui.unmount();
  });

  it('always says whether verification is skipped, even with tls off', async () => {
    const fetchMock = vi.fn(async () => ({ ok: false, status: 500, statusText: 'x', text: async () => '' }));
    vi.stubGlobal('fetch', fetchMock);
    const ui = mount(strip);
    ui.click('[aria-label="Share request"] .btn.is-primary');
    await settle();
    const sent = JSON.parse((fetchMock.mock.calls[0] as unknown as [string, { body: string }])[1].body);
    expect(sent.tls).toBe(false);
    expect(sent.tls_insecure).toBe(false);
    ui.unmount();
  });
});

describe('the list of open tabs', () => {
  beforeEach(() => {
    useStore.setState({
      tabs: [tab('a', 'alpha.gctf'), tab('b', 'beta.gctf'), tab('c', 'gamma.gctf')],
      activeTabId: 'a',
      tabListIntent: 0,
    } as never);
  });

  afterEach(() => { useStore.setState({ tabs: [], activeTabId: null, tabListIntent: 0 } as never); });

  it('comes back unfiltered when it is asked for again', () => {
    const ui = mount(strip);
    act(() => { useStore.getState().requestTabList(); });
    ui.type('.tab-list .field', 'beta');
    expect(ui.all('.tab-list-row')).toHaveLength(1);

    ui.key('.tab-list .field', 'Escape');
    act(() => { useStore.getState().requestTabList(); });
    expect(ui.all('.tab-list-row')).toHaveLength(3);
    expect((ui.get('.tab-list .field') as HTMLInputElement).value).toBe('');
    ui.unmount();
  });

  it('opens the row the highlight is on, after the list has narrowed under it', () => {
    const ui = mount(strip);
    act(() => { useStore.getState().requestTabList(); });
    ui.key('.tab-list .field', 'ArrowDown');
    ui.key('.tab-list .field', 'ArrowDown');
    ui.type('.tab-list .field', 'beta');
    expect(ui.all('.tab-list-row')).toHaveLength(1);
    expect(ui.get('.tab-list-row').className).toContain('is-cursor');
    ui.key('.tab-list .field', 'Enter');
    expect(useStore.getState().activeTabId).toBe('b');
    ui.unmount();
  });
});
