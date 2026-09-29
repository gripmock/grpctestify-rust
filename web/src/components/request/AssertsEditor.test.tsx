import { describe, expect, it, beforeEach, afterEach } from 'vitest';
import { act } from 'react';
import { AssertsEditor } from './AssertsEditor';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';

const VERDICT = {
  expression: '.expires_in == 3600',
  layer: 'ast',
  suggestion: null,
  passed: false,
  error: null,
  message: 'Assertion failed: .expires_in == 3600 (Values: "3600" vs 3600)',
  expected: '== 3600',
  actual: '"3600"',
  hint: 'the answer holds it as a string; compare with `.expires_in:number` (protobuf sends 64-bit integers that way)',
  elapsed_us: 12,
};

const editor = () => (
  <ToastProvider>
    <AssertsEditor asserts={['.expires_in == 3600']} />
  </ToastProvider>
);

describe('what a failed assertion says in the editor', () => {
  let original: typeof globalThis.fetch;

  beforeEach(() => {
    original = globalThis.fetch;
    globalThis.fetch = (async () => ({ ok: true, json: async () => [VERDICT] })) as any;
    useStore.setState({
      workspacePath: 'a.gctf',
      collectionParsed: null,
      documents: [],
      activeStep: 0,
      response: {
        status: 'ok',
        messages: [{ expires_in: '3600' }],
        headers: {},
        trailers: {},
        durationMs: 3,
      } as any,
    });
  });

  afterEach(() => { globalThis.fetch = original; });

  it('carries the remedy the engine sent', async () => {
    const ui = mount(editor());
    await act(async () => { await new Promise(r => setTimeout(r, 400)); });
    const said = ui.get('.assert-remedy');
    expect(said.textContent).toContain('.expires_in:number');
    expect(ui.get('.assert-why').textContent).toContain('"3600"');
    ui.unmount();
  });

  it('marks a line the file itself is refused for', async () => {
    useStore.setState({
      diagnostics: [{
        range: { start: { line: 8, character: 0 }, end: { line: 8, character: 8 } },
        message: 'Assertion ends on `==` with nothing to compare against: .name ==',
      }],
      diagnosedText: null,
    } as never);
    const ui = mount(
      <ToastProvider>
        <AssertsEditor asserts={['.name ==']} />
      </ToastProvider>,
    );
    await act(async () => { await new Promise(r => setTimeout(r, 400)); });
    expect(ui.get('.assert').className).toContain('is-fail');
    expect(ui.get('.assert-said').textContent).toContain('nothing to compare against');
    ui.unmount();
    useStore.setState({ diagnostics: [] } as never);
  });
});

describe('the verdicts on the lines already written', () => {
  let original: typeof globalThis.fetch;

  beforeEach(() => {
    original = globalThis.fetch;
    globalThis.fetch = (async () => ({ ok: true, json: async () => [VERDICT] })) as any;
    useStore.setState({
      workspacePath: 'a.gctf', collectionParsed: null, documents: [], activeStep: 0, diagnostics: [],
      response: {
        status: 'ok', messages: [{ expires_in: '3600' }], headers: {}, trailers: {}, durationMs: 3,
      } as never,
    });
  });

  afterEach(() => { globalThis.fetch = original; });

  it('stay on screen while a new line is being typed', async () => {
    const ui = mount(editor());
    await act(async () => { await new Promise(r => setTimeout(r, 400)); });
    expect(ui.get('.assert').className).toContain('is-fail');
    expect(ui.get('.assert-why').textContent).toContain('"3600"');

    ui.type('.field-frame .field', '.token != ""');
    expect(ui.get('.assert').className).toContain('is-fail');
    expect(ui.get('.assert-why').textContent).toContain('"3600"');
    ui.unmount();
  });
});
