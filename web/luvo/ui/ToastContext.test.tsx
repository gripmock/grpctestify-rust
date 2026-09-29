import { describe, expect, it } from 'vitest';
import { ToastProvider } from './ToastContext';
import { useToast } from './useToast';
import { mount } from 'luvo/test/render';

function Speaker() {
  const toast = useToast();
  return (
    <>
      <button className="ok" onClick={() => toast.success('saved')}>ok</button>
      <button className="fail" onClick={() => toast.error('not saved')}>fail</button>
      <button className="no" onClick={() => toast.refuse('this step has edits')}>no</button>
      <button className="note" onClick={() => toast.info('reloaded')}>note</button>
    </>
  );
}

const speaker = () => <ToastProvider><Speaker /></ToastProvider>;

describe('what a reader is told about a toast', () => {
  it('leaves the announcing to each toast, so nothing is said twice', () => {
    const ui = mount(speaker());
    ui.click('.ok');
    const stack = document.querySelector('.toasts');
    expect(stack?.getAttribute('aria-live')).toBeNull();
    expect(stack?.getAttribute('role')).toBeNull();
    expect(document.querySelector('.toast')?.getAttribute('role')).toBe('status');
    ui.unmount();
  });

  it('raises an error and a refusal as alerts, the rest as status', () => {
    const ui = mount(speaker());
    ui.click('.ok');
    ui.click('.fail');
    ui.click('.no');
    ui.click('.note');
    const roles = [...document.querySelectorAll('.toast')].map(t => [t.querySelector('.toast-text')?.textContent, t.getAttribute('role')]);
    expect(roles).toEqual([
      ['saved', 'status'],
      ['not saved', 'alert'],
      ['this step has edits', 'alert'],
      ['reloaded', 'status'],
    ]);
    ui.unmount();
  });
});
