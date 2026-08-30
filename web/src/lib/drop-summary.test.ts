import { describe, it, expect } from 'vitest';
import { summariseDrop, type DropOutcome } from './drop-summary';

const schema = (name: string): DropOutcome => ({ kind: 'schema', name });
const opened = (name: string): DropOutcome => ({ kind: 'opened', name });
const refused = (name: string, reason = 'not a .gctf or a schema'): DropOutcome =>
  ({ kind: 'refused', name, reason });

describe('summariseDrop', () => {
  it('says nothing for nothing', () => {
    expect(summariseDrop([])).toBeNull();
  });

  it('keeps a single file personal', () => {
    expect(summariseDrop([schema('auth.proto')])).toEqual({ text: 'auth.proto added to the collections', failed: false });
    expect(summariseDrop([opened('login.gctf')])).toEqual({ text: 'login.gctf opened as a tab', failed: false });
    expect(summariseDrop([refused('notes.txt')])).toEqual({ text: 'notes.txt: not a .gctf or a schema', failed: true });
  });

  it('counts a batch and names what it would not take', () => {
    expect(summariseDrop([schema('a.proto'), schema('b.proto'), opened('c.gctf'), refused('d.txt')]))
      .toEqual({ text: '2 schemas added · 1 file opened · 1 refused — d.txt', failed: true });
  });

  it('is not a failure when everything landed', () => {
    expect(summariseDrop([schema('a.proto'), schema('b.proto')])?.failed).toBe(false);
  });
});

describe('a schema dropped while a file is open', () => {
  it('says what picks it up', () => {
    const said = summariseDrop([{ kind: 'schema', name: 'api.desc' }], { fileOpen: true });
    expect(said?.text).toBe('api.desc added to the collections — pick it in the file’s PROTO section');
  });

  it('says it once for a batch', () => {
    const said = summariseDrop(
      [{ kind: 'schema', name: 'a.desc' }, { kind: 'schema', name: 'b.proto' }],
      { fileOpen: true },
    );
    expect(said?.text).toBe('2 schemas added — pick them in the file’s PROTO section');
  });

  it('says nothing about a section when no file is open', () => {
    expect(summariseDrop([{ kind: 'schema', name: 'api.desc' }])?.text)
      .toBe('api.desc added to the collections');
  });

  it('leaves an opened file alone', () => {
    expect(summariseDrop([{ kind: 'opened', name: 'login.gctf' }], { fileOpen: true })?.text)
      .toBe('login.gctf opened as a tab');
  });
});
