import { describe, it, expect } from 'vitest';
import { codeName, readErrorBody, writeErrorField } from './grpc-codes';

describe('readErrorBody', () => {
  it('reads the two fields the picker edits', () => {
    expect(readErrorBody('{"code": 5, "message": "no user"}')).toEqual({ code: 5, message: 'no user', extra: false });
  });

  it('flags anything the fields cannot express', () => {
    expect(readErrorBody('{"code": 5, "details": []}')?.extra).toBe(true);
    expect(readErrorBody('{"code": "NOT_FOUND"}')).toEqual({ code: null, message: null, extra: true });
  });

  it('answers null for what is not an object', () => {
    expect(readErrorBody('[]')).toBeNull();
    expect(readErrorBody('nonsense')).toBeNull();
  });
});

describe('writeErrorField', () => {
  it('keeps the rest of the body', () => {
    const out = writeErrorField('{"message": "gone", "details": []}', 'code', 5);
    expect(JSON.parse(out)).toEqual({ message: 'gone', details: [], code: 5 });
  });

  it('removes the field when it is cleared', () => {
    expect(JSON.parse(writeErrorField('{"code": 5, "message": "x"}', 'message', ''))).toEqual({ code: 5 });
  });
});

describe('codeName', () => {
  it('names the number the file carries', () => {
    expect(codeName(5)).toBe('NOT_FOUND');
    expect(codeName(99)).toBeNull();
  });
});
