import { describe, expect, it, beforeEach } from 'vitest';
import { DatasetEditor } from './DatasetEditor';
import { useStore } from '../../lib/store';
import type { CollectionParsed } from '../../lib/types';
import { mount } from 'luvo/test/render';
import { ToastProvider } from 'luvo/ui/ToastContext';

function parsed(over: Partial<CollectionParsed> = {}): CollectionParsed {
  return {
    endpoint: 'a.A/One', address: '', headers: {}, bodies: ['{}'],
    asserts: [], extracts: {}, meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null, meta_links: [],
    tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [],
    expect_responses: [], expect_error: null,
    ...over,
  };
}

function put(p: CollectionParsed) {
  useStore.setState({
    collectionParsed: p,
    workspaceOriginal: p,
    request: { endpoint: p.endpoint, headers: p.headers, bodies: p.bodies },
  });
}

describe('the dataset grid', () => {
  beforeEach(() => { put(parsed()); });

  it('counts the cases its rows make', () => {
    put(parsed({ dataset: [{ id: '1' }, { id: '2' }, { id: '3' }] }));
    const ui = mount(<ToastProvider><DatasetEditor /></ToastProvider>);
    expect(ui.get('.bar .field-label').textContent).toBe('3 rows → 3 cases');
    ui.unmount();
  });

  it('explains itself while it is empty', () => {
    const ui = mount(<ToastProvider><DatasetEditor /></ToastProvider>);
    expect(ui.get('.note').textContent).toContain('A row per case');
    expect(ui.all('.dataset-row')).toHaveLength(0);
    ui.unmount();
  });

  it('adds a column by name', () => {
    const ui = mount(<ToastProvider><DatasetEditor /></ToastProvider>);
    ui.type('.field-frame input', 'who');
    ui.click(ui.byText('add column')[0]);
    expect(useStore.getState().collectionParsed?.dataset).toEqual([{ who: '' }]);
    ui.unmount();
  });

  it('marks a column the file never reads', () => {
    put(parsed({ dataset: [{ who: 'Ada', spare: 'x' }], bodies: ['{"m":"{{dataset.who}}"}'] }));
    const ui = mount(<ToastProvider><DatasetEditor /></ToastProvider>);
    const unused = ui.all('.dataset-name.is-unused');
    expect(unused).toHaveLength(1);
    expect((unused[0] as HTMLInputElement).value).toBe('spare');
    ui.unmount();
  });

  it('names a placeholder with no column behind it', () => {
    put(parsed({ dataset: [{ who: 'Ada' }], bodies: ['{"m":"{{dataset.missing}}"}'] }));
    const ui = mount(<ToastProvider><DatasetEditor /></ToastProvider>);
    expect(ui.get('.note.is-warn').textContent).toContain('{{dataset.missing}}');
    ui.unmount();
  });

  it('names a placeholder written where nothing substitutes', () => {
    put(parsed({ dataset: [{ who: 'Ada' }], bodies: ['{}'], asserts: ['.m == "{{dataset.who}}"'] }));
    const ui = mount(<ToastProvider><DatasetEditor /></ToastProvider>);
    const warn = ui.all('.note.is-warn').map(n => n.textContent ?? '').join(' ');
    expect(warn).toContain('{{dataset.who}}');
    expect(warn).toContain('ASSERTS');
    ui.unmount();
  });

  it('renames a column without losing what its rows hold', () => {
    put(parsed({ dataset: [{ who: 'Ada' }, { who: 'Grace' }] }));
    const ui = mount(<ToastProvider><DatasetEditor /></ToastProvider>);
    const head = ui.get('.dataset-name') as HTMLInputElement;
    ui.type(head, 'name');
    ui.key(head, 'Enter');
    expect(useStore.getState().collectionParsed?.dataset).toEqual([{ name: 'Ada' }, { name: 'Grace' }]);
    ui.unmount();
  });
});

describe('renaming a column', () => {
  it('takes the requests references with it', () => {
    put(parsed({ endpoint: 'GET /v1/{{dataset.id}}', dataset: [{ id: '1' }] }));
    useStore.setState({
      workspacePath: 'rows.httf',
      request: { endpoint: 'GET /v1/{{dataset.id}}', headers: { 'x-id': '{{dataset.id}}' }, bodies: [] },
    } as never);

    const touched = useStore.getState().renameDatasetColumn('id', 'user');

    expect(touched).toBe(2);
    expect(useStore.getState().request.endpoint).toBe('GET /v1/{{dataset.user}}');
    expect(useStore.getState().request.headers['x-id']).toBe('{{dataset.user}}');
    expect(useStore.getState().collectionParsed!.dataset).toEqual([{ user: '1' }]);
  });
});
