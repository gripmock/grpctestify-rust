import { describe, expect, it } from 'vitest';
import { seedSaveName } from './save-name';

const paths = ['SayHello.gctf', 'SayHello-2.gctf', 'auth/login.gctf', 'probe.httf'];

describe('the name a new file starts on', () => {
  it('is the derived one when nothing holds it', () => {
    expect(seedSaveName({ base: 'Subscribe', ext: 'gctf', folder: '', paths }))
      .toEqual({ name: 'Subscribe', taken: null });
  });

  it('steps past a name the project already holds, and says which', () => {
    expect(seedSaveName({ base: 'SayHello', ext: 'gctf', folder: '', paths }))
      .toEqual({ name: 'SayHello-3', taken: 'SayHello.gctf' });
  });

  it('is per folder — the same name elsewhere is a different file', () => {
    expect(seedSaveName({ base: 'login', ext: 'gctf', folder: 'auth', paths }))
      .toEqual({ name: 'login-2', taken: 'auth/login.gctf' });
    expect(seedSaveName({ base: 'login', ext: 'gctf', folder: 'billing', paths }))
      .toEqual({ name: 'login', taken: null });
  });

  it('reads the family it is being saved as', () => {
    expect(seedSaveName({ base: 'probe', ext: 'gctf', folder: '', paths }))
      .toEqual({ name: 'probe', taken: null });
    expect(seedSaveName({ base: 'probe', ext: 'httf', folder: '', paths }))
      .toEqual({ name: 'probe-2', taken: 'probe.httf' });
  });

  it('has nothing to say about an empty name', () => {
    expect(seedSaveName({ base: '  ', ext: 'gctf', folder: '', paths })).toEqual({ name: '', taken: null });
  });
});
