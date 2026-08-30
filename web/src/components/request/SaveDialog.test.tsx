import { describe, expect, it, beforeEach } from 'vitest';
import { SaveDialog } from './SaveDialog';
import { useStore } from '../../lib/store';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { mount } from 'luvo/test/render';

const dialog = () => (
  <ToastProvider>
    <ModalProvider>
      <SaveDialog onClose={() => {}} onSave={async () => {}} />
    </ModalProvider>
  </ToastProvider>
);

describe('the family a save writes into', () => {
  beforeEach(() => {
    useStore.setState({ workspacePath: null, collections: [], collectionParsed: null });
  });

  it('says so when the request does not belong to it', () => {
    useStore.setState({ request: { endpoint: 'GET /v1/users', headers: {}, bodies: [] } });
    const ui = mount(dialog());
    ui.click('.save-family button:first-child');
    expect(ui.get('.save-family-note').textContent).toContain('a .gctf calls a service and a method');
    ui.unmount();
  });

  it('says nothing when they agree', () => {
    useStore.setState({ request: { endpoint: 'a.B/C', headers: {}, bodies: ['{}'] } });
    const ui = mount(dialog());
    expect(ui.container.querySelector('.save-family-note')).toBeNull();
    ui.unmount();
  });

  it('says nothing before an endpoint is typed', () => {
    useStore.setState({ request: { endpoint: '', headers: {}, bodies: ['{}'] } });
    const ui = mount(dialog());
    ui.click('.save-family button:last-child');
    expect(ui.container.querySelector('.save-family-note')).toBeNull();
    ui.unmount();
  });
});

describe('the name a save starts with', () => {
  beforeEach(() => {
    useStore.setState({ collections: [], collectionParsed: null, request: { endpoint: 'GET /v1/users', headers: {}, bodies: [] } });
  });

  it('drops the extension of the file it is saving', () => {
    useStore.setState({ workspacePath: 'api/probe.httf' });
    const ui = mount(dialog());
    expect(ui.get('.save-path').textContent).toBe('api/probe.httf');
    ui.unmount();
  });

  it('drops it for the other family too', () => {
    useStore.setState({ workspacePath: 'api/greet.gctf', request: { endpoint: 'a.B/C', headers: {}, bodies: ['{}'] } });
    const ui = mount(dialog());
    expect(ui.get('.save-path').textContent).toBe('api/greet.gctf');
    ui.unmount();
  });

  it('names an unsaved request after its path, in its own family', () => {
    useStore.setState({ workspacePath: null });
    const ui = mount(dialog());
    expect(ui.get('.save-path').textContent).toBe('users.httf');
    ui.unmount();
  });
});

describe('a preview the server refuses', () => {
  it('says why, and the save it would refuse is not offered', async () => {
    useStore.setState({
      request: { endpoint: 'a.B/C', headers: {}, bodies: ['not json'] },
      previewSave: async () => ({ error: 'message #1 is not valid JSON' }),
    } as never);
    const ui = mount(dialog());
    await new Promise(r => setTimeout(r, 220));
    expect(ui.get('.save-preview').textContent).toContain('message #1 is not valid JSON');
    const save = ui.all('button').find(b => /save/i.test(b.textContent ?? ''));
    expect((save as HTMLButtonElement).disabled).toBe(true);
    ui.unmount();
  });
});

describe('the name a save starts with', () => {
  it('is the last part of the path', () => {
    useStore.setState({ workspacePath: null, collections: [], collectionParsed: null,
      request: { endpoint: 'GET /v1/users?page=2', headers: {}, bodies: [] } });
    const ui = mount(dialog());
    const field = ui.all('input').find(i => (i as HTMLInputElement).placeholder === 'login');
    expect((field as HTMLInputElement).value).toBe('users');
    ui.unmount();
  });
});

describe('a file that names no address', () => {
  it('says who will aim it, and offers to write the target in', () => {
    useStore.setState({
      workspacePath: null, collections: [], collectionParsed: null,
      address: '', addressTouched: false, protocol: 'grpc',
      request: { endpoint: 'a.B/C', headers: {}, bodies: ['{}'] },
    });
    const ui = mount(dialog());
    expect(ui.container.textContent).toContain('takes it from the environment');
    const write = ui.all('button').find(b => /write .* into it/.test(b.textContent ?? ''));
    ui.click(write!);
    expect(useStore.getState().addressTouched).toBe(true);
    ui.unmount();
  });

  it('says nothing when the file will carry one', () => {
    useStore.setState({
      workspacePath: null, collections: [], collectionParsed: null,
      address: 'localhost:4770', addressTouched: true, protocol: 'grpc',
      request: { endpoint: 'a.B/C', headers: {}, bodies: ['{}'] },
    });
    const ui = mount(dialog());
    expect(ui.container.textContent).not.toContain('takes it from the environment');
    ui.unmount();
  });
});
