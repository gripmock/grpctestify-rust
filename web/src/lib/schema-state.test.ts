import { describe, expect, it } from 'vitest';
import { plainReason, schemaState, fileNames } from './schema-state';

const base = {
  reflectStatus: 'idle' as const,
  reflectError: null,
  methodCount: 0,
  protoSource: 'reflection' as const,
  protoFiles: 0,
};

describe('schemaState', () => {
  it('says it is asking while reflection is in flight', () => {
    expect(schemaState({ ...base, reflectStatus: 'loading' }).kind).toBe('inspecting');
  });

  it('names the server as the source once reflection answered', () => {
    const state = schemaState({ ...base, reflectStatus: 'ok', methodCount: 27, serviceCount: 4 });
    expect(state.kind).toBe('reflected');
    expect(state.label).toBe('27 methods · 4 services');
  });

  it('leaves the service count out when nothing counted them', () => {
    expect(schemaState({ ...base, reflectStatus: 'ok', methodCount: 27 }).label).toBe('27 methods');
  });

  it('prefers the file’s own schema over reflection, because the run does', () => {
    const state = schemaState({
      ...base,
      reflectStatus: 'ok',
      methodCount: 27,
      protoSource: 'files',
      protoFiles: 2,
    });
    expect(state.kind).toBe('files');
    expect(state.label).toBe('2 proto files');
  });

  it('carries the reason when reflection failed', () => {
    const state = schemaState({ ...base, reflectStatus: 'error', reflectError: 'connection refused' });
    expect(state.kind).toBe('error');
    expect(state.hint).toContain('connection refused');
  });

  it('says nobody has asked yet when nobody has', () => {
    const state = schemaState(base);
    expect(state.kind).toBe('unasked');
    expect(state.label).toBe('not asked yet');
    expect(state.hint).toContain('ask it');
  });

  it('says the server named nothing once it has answered', () => {
    const state = schemaState({ ...base, reflectStatus: 'ok' });
    expect(state.kind).toBe('none');
    expect(state.label).toBe('no methods');
    expect(state.hint).toContain('answered with nothing');
  });
});

describe('plainReason', () => {
  it('drops the prefix the line is about to add anyway', () => {
    expect(plainReason('Reflection failed: No descriptors loaded')).toBe('No descriptors loaded');
    expect(plainReason('reflection failed - x')).toBe('- x');
  });

  it('leaves an unrelated message alone, minus trailing punctuation', () => {
    expect(plainReason('the server answered 502 Bad Gateway')).toBe('the server answered 502 Bad Gateway');
    expect(plainReason('no answer in 30 s.')).toBe('no answer in 30 s');
    expect(plainReason(null)).toBe('');
  });
});

describe('the failed-reflection hint', () => {
  it('says the reason once', () => {
    const state = schemaState({
      reflectStatus: 'error',
      reflectError: 'Reflection failed: No descriptors loaded via reflection',
      methodCount: 0,
      protoSource: 'reflection',
      protoFiles: 0,
    });
    expect(state.hint).toBe('Reflection failed — No descriptors loaded via reflection. Point the file at a .proto instead.');
  });

  it('does not repeat advice the reason already carries', () => {
    const state = schemaState({
      reflectStatus: 'error',
      reflectError: 'the server reflected no methods — it may not serve reflection; a PROTO descriptor in the file works without it',
      methodCount: 0,
      protoSource: 'reflection',
      protoFiles: 0,
    });
    expect(state.hint).toBe('Reflection failed — the server reflected no methods — it may not serve reflection; a PROTO descriptor in the file works without it');
  });
});

describe('a reason that already names reflection', () => {
  it('is not prefixed with the failure again', () => {
    const state = schemaState({
      reflectStatus: 'error',
      reflectError: 'grpc-web and ConnectRPC have no reflection — name a descriptor in the file’s PROTO section',
      methods: [],
      proto: {},
    } as never);
    expect(state.hint).toBe('grpc-web and ConnectRPC have no reflection — name a descriptor in the file’s PROTO section');
  });
});

describe('the schema a file names', () => {
  it('names the file, not the path it sits at', () => {
    expect(fileNames('/p/.grpctestify/protos/api.desc')).toBe('api.desc');
    expect(fileNames('a/one.proto, b/two.proto')).toBe('one.proto, two.proto');
  });

  it('counts them once there are more than two', () => {
    expect(fileNames('a.proto, b.proto, c.proto, d.proto')).toBe('a.proto and 3 more');
  });

  it('says nothing when the section names nothing', () => {
    expect(fileNames('')).toBe('');
    expect(fileNames('  , ')).toBe('');
  });

  it('is the label beside what the source is', () => {
    const said = schemaState({
      reflectStatus: 'idle', reflectError: null, methodCount: 0,
      protoSource: 'descriptor', protoFiles: 0, protoNames: '/p/api.desc',
    });
    expect(said.label).toBe('descriptor set · api.desc');
  });
});

describe('how old the method list is', () => {
  const asked = {
    reflectStatus: 'ok' as const, reflectError: null, methodCount: 27, serviceCount: 4,
    protoSource: 'reflection' as const, protoFiles: 0,
  };

  it('says when the server answered', () => {
    const state = schemaState({ ...asked, reflectedAt: '12:04' });
    expect(state.hint).toContain('answered at 12:04');
    expect(state.hint).toContain('as it was then');
  });

  it('says what it always said when there is no time to give', () => {
    expect(schemaState({ ...asked }).hint).toBe('Reflection answered — the method list is the server’s own');
  });

  it('leaves the label alone', () => {
    expect(schemaState({ ...asked, reflectedAt: '12:04' }).label).toBe('27 methods · 4 services');
  });
});
