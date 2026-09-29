import { describe, expect, it, beforeEach } from 'vitest';
import { ExpectEditor } from './ExpectEditor';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { useStore } from '../../lib/store';
import type { CollectionParsed } from '../../lib/types';
import { mount } from 'luvo/test/render';

function parsed(over: Partial<CollectionParsed> = {}): CollectionParsed {
  return {
    endpoint: 'a.A/One', address: '', headers: {}, bodies: ['{}'],
    asserts: [], extracts: {}, meta_name: null, meta_tags: [], meta_owner: null, meta_summary: null, meta_links: [],
    tls: {}, options: {}, bench: {}, proto: {}, dataset: [], attributes: [],
    expect_responses: [], expect_error: null,
    ...over,
  };
}

const editor = (p: CollectionParsed) => (
  <ToastProvider>
    <ExpectEditor parsed={p} />
  </ToastProvider>
);

const modes = (ui: ReturnType<typeof mount>) =>
  ui.all('.expect-modes .seg button').map(b => b.textContent?.trim());

describe('what a step must come back with', () => {
  beforeEach(() => {
    useStore.setState({ workspacePath: 'a.gctf', collectionParsed: null, workspaceOriginal: null });
  });

  it('offers a gRPC file all three answers', () => {
    const ui = mount(editor(parsed()));
    expect(modes(ui)).toEqual(['asserts only', 'response', 'error']);
    ui.unmount();
  });

  it('does not offer an HTTP file an error to expect', () => {
    useStore.setState({ workspacePath: 'a.httf' });
    const ui = mount(editor(parsed()));
    expect(modes(ui)).toEqual(['asserts only', 'response']);
    ui.unmount();
  });

  it('says what to do instead for a file that already has one', () => {
    useStore.setState({
      workspacePath: 'a.httf',
      collectionParsed: parsed({ expect_error: { body: '{"code": 5}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] } }),
    });
    const ui = mount(editor(useStore.getState().collectionParsed!));
    expect(ui.get('.note.is-warn').textContent).toContain('@status() == 404');
    ui.unmount();
  });
});

describe('expecting no messages', () => {
  it('says what an empty block asserts', () => {
    useStore.setState({
      workspacePath: 'a.gctf',
      collectionParsed: parsed({
        expect_responses: [{ body: '', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }],
      }),
    });
    const ui = mount(editor(useStore.getState().collectionParsed!));
    expect(ui.get('.note').textContent).toContain('no messages');
    ui.unmount();
  });

  it('offers one beside a gRPC expectation that has a body', () => {
    useStore.setState({
      workspacePath: 'a.gctf',
      collectionParsed: parsed({
        expect_responses: [{ body: '{"ok": true}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }],
      }),
    });
    const ui = mount(editor(useStore.getState().collectionParsed!));
    const offered = ui.all('button').map(b => b.textContent?.trim());
    expect(offered).toContain('no messages');
    ui.unmount();
  });

  it('does not offer one to an HTTP file', () => {
    useStore.setState({
      workspacePath: 'a.httf',
      request: { endpoint: 'GET /a', headers: {}, bodies: [] },
      collectionParsed: parsed({
        endpoint: 'GET /a',
        expect_responses: [{ body: 'words', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }],
      }),
    });
    const ui = mount(editor(useStore.getState().collectionParsed!));
    const offered = ui.all('button').map(b => b.textContent?.trim());
    expect(offered).not.toContain('no messages');
    ui.unmount();
  });
});

describe('an HTTP expectation', () => {
  it('offers one body and calls it a body', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      request: { endpoint: 'GET /text', headers: {}, bodies: [] },
      collectionParsed: parsed({
        endpoint: 'GET /text',
        expect_responses: [{ body: 'plain words here', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }],
      }),
    });
    const ui = mount(editor(useStore.getState().collectionParsed!));
    expect(ui.container.textContent).toContain('expected body');
    expect(ui.container.querySelector('.note.is-warn')).toBeNull();
    ui.unmount();
  });
});

describe('a text body', () => {
  it('is not offered the rules that read JSON', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      request: { endpoint: 'GET /text', headers: {}, bodies: [] },
      collectionParsed: parsed({
        endpoint: 'GET /text',
        expect_responses: [{ body: 'plain words here', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }],
      }),
    });
    const ui = mount(editor(useStore.getState().collectionParsed!));
    expect(ui.container.textContent).not.toContain('unordered arrays');
    ui.unmount();
  });

  it('keeps them for a JSON one', () => {
    useStore.setState({
      workspacePath: 'probe.httf',
      request: { endpoint: 'GET /j', headers: {}, bodies: [] },
      collectionParsed: parsed({
        endpoint: 'GET /j',
        expect_responses: [{ body: '{"a":1}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }],
      }),
    });
    const ui = mount(editor(useStore.getState().collectionParsed!));
    expect(ui.container.textContent).toContain('unordered arrays');
    ui.unmount();
  });
});

describe('an answer that belongs to another step', () => {
  const answered = (fromStep: number) => ({
    status: 'ok' as const, statusCode: 200, messages: [{ name: 'Ada' }],
    headers: {}, trailers: {}, error: null, durationMs: 1, fromStep,
  });

  it('is not offered as this step\'s body', () => {
    useStore.setState({
      workspacePath: 'chain.httf',
      documents: [{ index: 0 }, { index: 1 }] as never,
      activeStep: 0,
      response: answered(1),
    } as never);
    const ui = mount(editor(parsed({ expect_responses: [{ body: '{}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }] })));
    expect(ui.all('button').some(b => /from the answer/i.test(b.textContent ?? ''))).toBe(false);
    ui.unmount();
  });

  it('is offered when it is this step\'s', () => {
    useStore.setState({
      workspacePath: 'chain.httf',
      documents: [{ index: 0 }, { index: 1 }] as never,
      activeStep: 1,
      response: answered(1),
    } as never);
    const ui = mount(editor(parsed({ expect_responses: [{ body: '{}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }] })));
    expect(ui.all('button').some(b => /from the answer/i.test(b.textContent ?? ''))).toBe(true);
    ui.unmount();
  });
});

describe('the message a failure must carry', () => {
  it('is held in a control that keeps its newlines', () => {
    useStore.setState({
      workspacePath: 'a.gctf',
      request: { endpoint: 'a.A/One', headers: {}, bodies: ['{}'] },
      collectionParsed: parsed({
        expect_error: {
          body: JSON.stringify({ code: 5, message: 'No matching stub found\nService: a.A' }),
          partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [],
        },
      }),
    });
    const ui = mount(editor(useStore.getState().collectionParsed!));
    const field = ui.container.querySelector('textarea.expect-error-text') as HTMLTextAreaElement;
    expect(field?.tagName).toBe('TEXTAREA');
    expect(field.value).toBe('No matching stub found\nService: a.A');
    ui.unmount();
  });
});

describe('an HTTP file that checks no status', () => {
  beforeEach(() => {
    useStore.setState({ workspacePath: 'api/users.httf', response: null });
  });

  const note = (ui: ReturnType<typeof mount>) =>
    ui.all('.note').map(n => n.textContent ?? '').find(t => t.includes('Nothing here checks the status'));

  it('is told, once it expects anything at all', () => {
    const ui = mount(editor(parsed({ asserts: ['.name == "Ada"'] })));
    expect(note(ui)).toBeTruthy();
    ui.unmount();
  });

  it('is not told about a file that has no expectation yet', () => {
    const ui = mount(editor(parsed()));
    expect(note(ui)).toBeUndefined();
    ui.unmount();
  });

  it('is not told twice: a file that checks it says nothing', () => {
    const ui = mount(editor(parsed({ asserts: ['@status() == 200'] })));
    expect(note(ui)).toBeUndefined();
    ui.unmount();
  });

  it('is not told about a .gctf', () => {
    useStore.setState({ workspacePath: 'a.gctf' });
    const ui = mount(editor(parsed({ asserts: ['.name == "Ada"'] })));
    expect(note(ui)).toBeUndefined();
    ui.unmount();
  });

  it('offers the code the last call answered', () => {
    useStore.setState({
      response: { status: 'ok', statusCode: 201, messages: [{}], headers: {}, trailers: {}, error: null, durationMs: 2 } as any,
    });
    const ui = mount(editor(parsed({ asserts: ['.name == "Ada"'] })));
    expect(ui.all('button').some(b => b.textContent === '@status() == 201')).toBe(true);
    ui.unmount();
  });
});

describe('what a run would fail on', () => {
  const answered = (over: Record<string, unknown>) => ({
    status: 'ok', statusCode: 500, messages: [{}], headers: {}, trailers: {}, error: null, durationMs: 2, ...over,
  }) as any;

  const said = (ui: ReturnType<typeof mount>) =>
    ui.all('.note').map(n => n.textContent ?? '').find(t => t.includes('a run would fail here'));

  it('is not a status an HTTP file does not check', () => {
    useStore.setState({ workspacePath: 'api/users.httf', response: answered({}) });
    const ui = mount(editor(parsed({ expect_responses: [{ body: '{}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }] })));
    expect(said(ui)).toBeUndefined();
    ui.unmount();
  });

  it('is a call that never arrived', () => {
    useStore.setState({
      workspacePath: 'api/users.httf',
      response: answered({ status: 'error', statusCode: null, error: 'connection refused' }),
    });
    const ui = mount(editor(parsed({ expect_responses: [{ body: '{}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }] })));
    expect(said(ui)).toBeTruthy();
    ui.unmount();
  });

  it('is a non-zero status on a .gctf', () => {
    useStore.setState({ workspacePath: 'a.gctf', response: answered({ statusCode: 5 }) });
    const ui = mount(editor(parsed({ expect_responses: [{ body: '{}', partial: false, unordered_arrays: false, with_asserts: false, tolerance: null, redact: [] }] })));
    expect(said(ui)).toBeTruthy();
    ui.unmount();
  });
});
