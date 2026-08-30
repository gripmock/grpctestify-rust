import { describe, it, expect } from 'vitest';
import { burstKey, burstRepeats, callSummary, dayMark, groupByDay, methodOf, payloadPreview, serviceOf, tookRange } from './history-group';

const at = (y: number, m: number, d: number, h = 12) => new Date(y, m, d, h).getTime();
const now = at(2026, 7, 21, 15);

describe('history reads by day', () => {
  it('names today and yesterday, and dates the rest', () => {
    const groups = groupByDay(
      [{ timestamp: at(2026, 7, 21, 9) }, { timestamp: at(2026, 7, 20, 9) }, { timestamp: at(2026, 7, 12, 9) }],
      now,
    );
    expect(groups.map(g => g.label)).toEqual(['today', 'yesterday', '12 aug']);
  });

  it('spells the year out when it is not this one', () => {
    const [g] = groupByDay([{ timestamp: at(2025, 0, 3, 9) }], now);
    expect(g.label).toBe('3 jan 2025');
  });

  it('keeps the order it was given and puts one day in one group', () => {
    const groups = groupByDay(
      [{ timestamp: at(2026, 7, 21, 15) }, { timestamp: at(2026, 7, 21, 9) }, { timestamp: at(2026, 7, 20, 23) }],
      now,
    );
    expect(groups.map(g => g.entries.length)).toEqual([2, 1]);
  });

  it('has nothing to say about nothing', () => {
    expect(groupByDay([], now)).toEqual([]);
  });
});

describe('splitting an endpoint', () => {
  it('takes the method and the service apart', () => {
    expect(methodOf('auth.v1.AuthService/Login')).toBe('Login');
    expect(serviceOf('auth.v1.AuthService/Login')).toBe('auth.v1.AuthService');
  });

  it('survives an endpoint with no service', () => {
    expect(methodOf('Login')).toBe('Login');
    expect(serviceOf('Login')).toBe('');
  });

  it('ignores a leading slash', () => {
    expect(methodOf('/auth.v1.AuthService/Login')).toBe('Login');
    expect(serviceOf('/auth.v1.AuthService/Login')).toBe('auth.v1.AuthService');
  });
});

describe('burstRepeats', () => {
  const call = (endpoint: string, status: string) => ({ endpoint, status });
  const key = (c: { endpoint: string; status: string }) => `${c.endpoint}|${c.status}`;

  it('folds consecutive repeats and keeps the order', () => {
    const got = burstRepeats(
      [call('a', 'ok'), call('a', 'ok'), call('b', 'ok'), call('a', 'ok')],
      key,
    );
    expect(got.map(g => [g.key, g.entries.length])).toEqual([
      ['a|ok', 2],
      ['b|ok', 1],
      ['a|ok', 1],
    ]);
  });

  it('keeps a repeat that ended differently apart', () => {
    const got = burstRepeats([call('a', 'ok'), call('a', 'error')], key);
    expect(got).toHaveLength(2);
  });

  it('has nothing to fold in an empty list', () => {
    expect(burstRepeats([], key)).toEqual([]);
  });
});

describe('tookRange', () => {
  it('says one number when they agree and a range when they do not', () => {
    expect(tookRange([6])).toBe('6 ms');
    expect(tookRange([6, 6, 6])).toBe('6 ms');
    expect(tookRange([47, 6, 12])).toBe('6–47 ms');
  });

  it('reads a slow burst the way a single slow call reads', () => {
    expect(tookRange([20_000, 21_000])).toBe('20–21 s');
    expect(tookRange([900, 20_000])).toBe('900 ms – 20 s');
  });

  it('is nothing when no call was timed', () => {
    expect(tookRange([])).toBeNull();
    expect(tookRange([null, undefined])).toBeNull();
  });
});

describe('payloadPreview', () => {
  it('is the message, without the pretty-printing', () => {
    expect(payloadPreview(['{\n  "email": "a@b.io"\n}'])).toBe('{"email":"a@b.io"}');
  });

  it('cuts what will not fit', () => {
    expect(payloadPreview(['{"a":"' + 'x'.repeat(80) + '"}'], 20)).toHaveLength(20);
  });

  it('says nothing when there is nothing to say', () => {
    expect(payloadPreview([])).toBe('');
    expect(payloadPreview(['   '])).toBe('');
  });

  it('keeps a non-JSON body readable', () => {
    expect(payloadPreview(['not   json\nat all'])).toBe('not json at all');
  });
});

describe('dayMark', () => {
  const noon = new Date(2026, 2, 3, 12, 0, 0).getTime();

  it('says nothing for today — the clock already does', () => {
    expect(dayMark(noon - 3600_000, noon)).toBeNull();
  });

  it('names yesterday and the days before it', () => {
    expect(dayMark(noon - 86_400_000, noon)).toBe('yesterday');
    expect(dayMark(new Date(2026, 1, 20, 9, 0, 0).getTime(), noon)).toBe('20 feb');
  });
});

describe('callSummary', () => {
  const ok = (bodies: string[], messages?: unknown[]) => ({ bodies, response: { status: 'ok', messages } });

  it('is the message that was sent', () => {
    expect(callSummary(ok(['{"email":"a@b.io"}']))).toEqual({ text: '{"email":"a@b.io"}', from: 'request' });
  });

  it('falls back to what came back, marked as such', () => {
    expect(callSummary(ok(['{}'], [{ token: 'tok-1' }]))).toEqual({ text: '{"token":"tok-1"}', from: 'response' });
  });

  it('is the reason when the call failed', () => {
    expect(callSummary({ bodies: ['{"a":1}'], response: { status: 'error', error: 'NotFound: no user\nat line 2' } }))
      .toEqual({ text: 'NotFound: no user', from: 'error' });
  });

  it('says failed when the failure said nothing', () => {
    expect(callSummary({ bodies: [], response: { status: 'error', error: null } }))
      .toEqual({ text: 'failed', from: 'error' });
  });

  it('keeps `{}` when there is nothing on either side', () => {
    expect(callSummary(ok(['{}'], []))).toEqual({ text: '{}', from: 'request' });
  });
});

describe('a failure in the rail', () => {
  it('drops the dialler prefix that was most of the line', () => {
    const line = callSummary({
      bodies: ['{}'],
      response: { status: 'error', error: 'gRPC error code=5 message=No matching stub found' },
    });
    expect(line).toEqual({ text: 'No matching stub found', from: 'error' });
  });

  it('says `failed` when the failure carried no words at all', () => {
    expect(callSummary({ bodies: [], response: { status: 'error', error: '' } }).text).toBe('failed');
  });
});

describe('what a history row calls a request', () => {
  it('is the method of a gRPC call', () => {
    expect(methodOf('users.UserService/GetUser')).toBe('GetUser');
  });

  it('is the verb and the path of an HTTP call', () => {
    expect(methodOf('GET /v1/users/7')).toBe('GET /v1/users/7');
    expect(methodOf('post /data.json')).toBe('POST /data.json');
  });

  it('drops the origin of an absolute url, which the row has no room for', () => {
    expect(methodOf('GET http://127.0.0.1:8099/data.json')).toBe('GET /data.json');
    expect(methodOf('GET https://api.example.com')).toBe('GET /');
  });
});

describe('burstKey', () => {
  const call = (over: Record<string, unknown> = {}) => ({
    endpoint: 'GET /v1/users',
    bodies: [''],
    response: { status: 'ok' },
    ...over,
  }) as Parameters<typeof burstKey>[0];

  it('folds the same call to the same place', () => {
    expect(burstKey(call({ connection: { address: 'a:1' } })))
      .toBe(burstKey(call({ connection: { address: 'a:1' } })));
  });

  it('keeps two servers apart — one row cannot say both addresses', () => {
    expect(burstKey(call({ connection: { address: 'a:1' } })))
      .not.toBe(burstKey(call({ connection: { address: 'b:2' } })));
  });

  it('keeps a plaintext call apart from the same call over TLS', () => {
    expect(burstKey(call({ connection: { address: 'a:1', tls: true } })))
      .not.toBe(burstKey(call({ connection: { address: 'a:1' } })));
  });

  it('still folds calls recorded before the connection was kept', () => {
    expect(burstKey(call())).toBe(burstKey(call({ connection: null })));
  });

  it('keeps a call that resolved a name apart from one that sent the braces', () => {
    expect(burstKey(call({ resolved: ['who'] }))).not.toBe(burstKey(call()));
  });

  it('folds two calls that resolved the same names', () => {
    expect(burstKey(call({ resolved: ['who', 'token'] })))
      .toBe(burstKey(call({ resolved: ['token', 'who'] })));
  });
});

describe('a failed call in the list', () => {
  const failed = (error: string, address?: string) => callSummary({
    bodies: ['{}'],
    response: { status: 'error', error, messages: [] },
    ...(address ? { connection: { address } } : {}),
  });

  it('reads as the failure card reads', () => {
    const line = failed(
      'Internal protocol error: received message with invalid compression flag: 73 (valid flags are 0 and 1) while receiving response with status: 403 Forbidden',
      'localhost:8871',
    );
    expect(line).toEqual({ text: 'localhost:8871 answered, but not as gRPC', from: 'error' });
  });

  it('drops the dialler’s prefix as it always did', () => {
    expect(failed('gRPC error code=5 message=Method not found').text).toBe('Method not found');
  });

  it('says something when the failure says nothing', () => {
    expect(failed('').text).toBe('failed');
  });
});

describe('two rows of one file', () => {
  const call = (row: number) => ({
    endpoint: 'pkg.Svc/M',
    bodies: ['{"name": "{{dataset.who}}"}'],
    response: { status: 'ok' },
    datasetRow: row,
  });

  it('are two calls', () => {
    expect(burstKey(call(0))).not.toBe(burstKey(call(1)));
  });

  it('and the same row twice is still one thing that happened twice', () => {
    expect(burstKey(call(1))).toBe(burstKey(call(1)));
  });
});
