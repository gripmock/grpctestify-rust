import { describe, it, expect } from 'vitest';
import { startHint, startSteps } from './start-here';

const base = { endpoint: '', methodCount: 0, fileCount: 0, hasWorkspaceFile: false, address: 'localhost:4770' };

describe('startHint', () => {
  it('is ready once an endpoint is named', () => {
    const hint = startHint({ ...base, endpoint: 'pkg.Svc/M' });
    expect(hint).toEqual({ title: 'no response yet', hint: null, ready: true, ask: false, bring: false });
  });

  it('points at the method list when reflection answered', () => {
    const hint = startHint({ ...base, methodCount: 12 });
    expect(hint.ready).toBe(false);
    expect(hint.hint).toContain('12 came back from localhost:4770');
  });

  it('says the open file names no endpoint, rather than blaming the user', () => {
    expect(startHint({ ...base, hasWorkspaceFile: true }).title).toBe('this file names no endpoint');
  });

  it('sends a first-timer to the rail when the project has files', () => {
    expect(startHint({ ...base, fileCount: 4 }).hint).toContain('Open a file from the rail');
  });

  it('names every way in when there is nothing at all', () => {
    const hint = startHint({ ...base, address: '' });
    expect(hint.hint).toContain('Type an endpoint');
    expect(hint.hint).toContain('drop a file');
    expect(hint.hint).not.toContain('curl');
    expect(hint.bring).toBe(true);
  });

  it('offers a command as a way in while nothing is typed', () => {
    expect(startHint({ ...base, fileCount: 12 }).bring).toBe(true);
  });

  it('does not offer one over a file that names no endpoint', () => {
    expect(startHint({ ...base, hasWorkspaceFile: true }).bring).toBe(false);
  });

  it('stops offering one once reflection has answered', () => {
    expect(startHint({ ...base, methodCount: 12 }).bring).toBe(false);
  });
});

describe('the first move in an empty workbench', () => {
  const base = { endpoint: '', methodCount: 0, fileCount: 0, hasWorkspaceFile: false, address: 'localhost:4770' };

  it('offers to ask the target', () => {
    expect(startHint(base).ask).toBe(true);
    expect(startHint({ ...base, fileCount: 4 }).ask).toBe(true);
  });

  it('does not offer it once there is a method, a list, or no address', () => {
    expect(startHint({ ...base, endpoint: 'a.A/One' }).ask).toBe(false);
    expect(startHint({ ...base, methodCount: 12 }).ask).toBe(false);
    expect(startHint({ ...base, address: '' }).ask).toBe(false);
  });
});

describe('startSteps', () => {
  const at = (over: Partial<Parameters<typeof startSteps>[0]> = {}) =>
    startSteps({ endpoint: '', methodCount: 0, address: 'localhost:4770', reachable: null, ...over });

  it('is three steps, in the order they happen', () => {
    expect(at().map(s => s.key)).toEqual(['target', 'method', 'send']);
  });

  it('counts a target as done once there is one and nothing said otherwise', () => {
    expect(at()[0].done).toBe(true);
    expect(at({ address: '' })[0].done).toBe(false);
  });

  it('is not done when nothing answered there', () => {
    const step = at({ reachable: false })[0];
    expect(step.done).toBe(false);
    expect(step.detail).toContain('nothing answered');
  });

  it('says how many methods there are to pick from, until one is picked', () => {
    expect(at({ methodCount: 4 })[1].detail).toBe('4 methods to pick from');
    expect(at({ endpoint: 'pkg.Svc/M' })[1]).toMatchObject({ done: true, detail: 'pkg.Svc/M' });
  });
});

describe('when the target refuses reflection', () => {
  const base = { endpoint: '', methodCount: 0, fileCount: 3, hasWorkspaceFile: false, address: 'localhost:50051' };

  it('stops offering the ask that just failed', () => {
    expect(startHint({ ...base, reflectionRefused: true }).ask).toBe(false);
  });

  it('says what happened and what is left to do', () => {
    const hint = startHint({ ...base, reflectionRefused: true });
    expect(hint.title).toContain('did not say what it serves');
    expect(hint.hint).toContain('localhost:50051');
    expect(hint.hint).toContain('PROTO');
  });

  it('still offers the ask before anything has been tried', () => {
    expect(startHint(base).ask).toBe(true);
  });

  it('gets out of the way once a method is named', () => {
    const hint = startHint({ ...base, endpoint: 'pkg.Svc/M', reflectionRefused: true });
    expect(hint.ready).toBe(true);
  });
});

describe('the second family in the first-run panel', () => {
  it('says an endpoint may be a path as readily as a method', () => {
    const steps = startSteps({ endpoint: '', methodCount: 0, address: 'localhost:4770', reachable: null });
    expect(steps.find(s => s.key === 'method')?.detail).toContain('GET /a/path');
  });

  it('says nothing extra once something has been typed', () => {
    const steps = startSteps({ endpoint: 'GET /v1/users', methodCount: 0, address: '', reachable: null });
    expect(steps.find(s => s.key === 'method')?.detail).toBe('GET /v1/users');
  });
});

describe('the first step of a first call', () => {
  const steps = (over: Partial<Parameters<typeof startSteps>[0]> = {}) =>
    startSteps({ endpoint: '', methodCount: 0, address: 'localhost:4770', reachable: null, ...over });

  it('says when the address is only the transport default', () => {
    const [target] = steps({ defaulted: true });
    expect(target.detail).toContain('where a gRPC call goes when nothing else names one');
  });

  it('says the address plainly when something named it', () => {
    const [target] = steps({ defaulted: false });
    expect(target.detail).toBe('localhost:4770');
  });

  it('is not done with no address at all', () => {
    const [target] = steps({ address: '', defaulted: false });
    expect(target.done).toBe(false);
    expect(target.detail).toBe('no address yet');
  });
});
