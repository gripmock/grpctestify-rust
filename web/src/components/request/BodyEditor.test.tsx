import { describe, expect, it } from 'vitest';
import { BodyEditor } from './BodyEditor';
import { useStore } from '../../lib/store';
import { mount } from 'luvo/test/render';

const http = (bodies: string[], headers: Record<string, string> = {}) => {
  useStore.setState({
    workspacePath: 'p.httf',
    request: { endpoint: 'POST /v1/users', headers, bodies },
  });
};

describe('what an HTTP request says it carries', () => {
  it('names the type inferred from the body', () => {
    http(['{"a":1}']);
    const ui = mount(<BodyEditor />);
    expect(ui.get('.content-type').textContent).toContain('application/json · inferred');
    ui.unmount();

    http(['name=Ada&age=36']);
    const form = mount(<BodyEditor />);
    expect(form.get('.content-type').textContent).toContain('application/x-www-form-urlencoded');
    form.unmount();
  });

  it('states the one the request names, as a fact rather than a guess', () => {
    http(['<user/>'], { 'Content-Type': 'application/xml' });
    const ui = mount(<BodyEditor />);
    expect(ui.get('.content-type').textContent).toBe('application/xml');
    expect(ui.get('.content-type').className).not.toContain('is-guess');
    ui.unmount();
  });

  it('writes the inferred one into the request when it is clicked', () => {
    http(['name=Ada']);
    const ui = mount(<BodyEditor />);
    ui.click('.content-type.is-guess');
    expect(useStore.getState().request.headers['content-type']).toBe('application/x-www-form-urlencoded');
    ui.unmount();
  });

  it('says nothing about a request with no body', () => {
    http([]);
    const ui = mount(<BodyEditor />);
    expect(ui.container.querySelector('.content-type')).toBeNull();
    ui.unmount();
  });
});

describe('an HTTP body that is not JSON', () => {
  it('is not marked as broken', () => {
    http(['name=Ada&age=36']);
    const ui = mount(<BodyEditor />);
    expect(ui.container.querySelector('.msg.is-bad')).toBeNull();
    expect(ui.container.textContent).not.toContain('not JSON —');
    ui.unmount();
  });

  it('still has nothing to indent', () => {
    http(['name=Ada&age=36']);
    const ui = mount(<BodyEditor />);
    const format = ui.all('button').find(b => b.textContent?.trim() === 'format');
    expect((format as HTMLButtonElement).disabled).toBe(true);
    ui.unmount();
  });

  it('leaves a gRPC message marked when it will not parse', () => {
    useStore.setState({
      workspacePath: 'a.gctf',
      request: { endpoint: 'a.B/C', headers: {}, bodies: ['not json'] },
    });
    const ui = mount(<BodyEditor />);
    expect(ui.container.querySelector('.msg.is-bad')).not.toBeNull();
    ui.unmount();
  });
});

describe('a form body', () => {
  it('is offered as the fields it holds', () => {
    http(['name=Ada&age=36']);
    const ui = mount(<BodyEditor />);
    const fields = ui.all('button').find(b => /fields/.test(b.textContent ?? ''));
    expect(fields).toBeTruthy();
    ui.click(fields!);
    const names = ui.all('input').map(i => (i as HTMLInputElement).value);
    expect(names).toContain('name');
    expect(names).toContain('Ada');
    ui.unmount();
  });

  it('writes what the rows say back into the body', () => {
    http(['name=Ada']);
    const ui = mount(<BodyEditor />);
    ui.click(ui.all('button').find(b => /fields/.test(b.textContent ?? ''))!);
    const value = ui.all('input').find(i => (i as HTMLInputElement).value === 'Ada')!;
    ui.type(value, 'Grace');
    expect(useStore.getState().request.bodies[0]).toBe('name=Grace');
    ui.unmount();
  });

  it('is not offered for a JSON one', () => {
    http(['{"a":1}']);
    const ui = mount(<BodyEditor />);
    expect(ui.all('button').some(b => /fields/.test(b.textContent ?? ''))).toBe(false);
    ui.unmount();
  });
});
