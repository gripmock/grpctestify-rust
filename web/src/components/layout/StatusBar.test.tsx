import { describe, expect, it, beforeEach } from 'vitest';
import { StatusBar } from './StatusBar';
import { ToastProvider } from 'luvo/ui/ToastContext';
import { ModalProvider } from 'luvo/ui/ModalContext';
import { mount } from 'luvo/test/render';
import { useStore } from '../../lib/store';

const bar = () => (
  <ToastProvider>
    <ModalProvider>
      <StatusBar />
    </ModalProvider>
  </ToastProvider>
);

describe('the project badge', () => {
  beforeEach(() => {
    useStore.setState({
      projectRoot: './.grpctestify',
      projectRootAbs: '/work/api/.grpctestify',
      collectionsDir: './.grpctestify/collections',
      projectEnvNames: ['example', 'staging'],
    });
  });

  it('names the project it is serving, in full', () => {
    const ui = mount(bar());
    const said = ui.get('.status-project').title;
    expect(said).toContain('/work/api/.grpctestify');
    expect(said).toContain('./.grpctestify/collections');
    expect(said).toContain('example, staging');
    ui.unmount();
  });

  it('falls back to the path the workbench was started with', () => {
    useStore.setState({ projectRootAbs: null });
    const ui = mount(bar());
    expect(ui.get('.status-project').title).toContain('./.grpctestify');
    ui.unmount();
  });

  it('says so when the project has no environments yet', () => {
    useStore.setState({ projectEnvNames: [] });
    expect(mount(bar()).get('.status-project').title).toContain('No environments yet');
  });

  it('is not there at all without a project', () => {
    useStore.setState({ projectRoot: null });
    const ui = mount(bar());
    expect(ui.all('.status-project')).toEqual([]);
    ui.unmount();
  });
});
