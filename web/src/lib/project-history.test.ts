import { describe, it, expect } from 'vitest';
import { flattenProjectHistory } from './project-history';
import { entryFailed } from './call-outcome';

describe('flattenProjectHistory', () => {
  const payload = {
    abc123: [
      { id: '1', timestamp: 10, endpoint: 'pkg.Svc/A', bodies: ['{}'], headers: { 'x-a': '1' }, response: { status: 'ok', messages: [{}] } },
      { id: '2', timestamp: 30, kind: 'run', collection_path: 'auth/login.gctf', response: { status: 'error', error: 'boom' } },
    ],
    def456: [
      { id: '3', timestamp: 20, endpoint: 'pkg.Svc/B', response: { status: 'ok' } },
    ],
  };

  it('merges every session into one list, newest first', () => {
    const rows = flattenProjectHistory(payload);
    expect(rows.map(r => r.id)).toEqual(['2', '3', '1']);
    expect(rows.map(r => r.session)).toEqual(['abc123', 'def456', 'abc123']);
  });

  it('gives a run its file as the name it is known by', () => {
    const run = flattenProjectHistory(payload).find(r => r.id === '2')!;
    expect(run.kind).toBe('run');
    expect(run.endpoint).toBe('auth/login.gctf');
    expect(run.collectionPath).toBe('auth/login.gctf');
    expect(run.response.status).toBe('error');
  });

  it('fills in what a line does not carry rather than trusting it', () => {
    const thin = flattenProjectHistory({ s: [{ timestamp: 1 }] })[0];
    expect(thin.bodies).toEqual([]);
    expect(thin.headers).toEqual({});
    expect(thin.response.messages).toEqual([]);
    expect(thin.id).toBeTruthy();
  });

  it('is empty for anything that is not a session map', () => {
    expect(flattenProjectHistory(null)).toEqual([]);
    expect(flattenProjectHistory({ s: 'not an array' })).toEqual([]);
  });
});

describe('a line the project recorded', () => {
  it('keeps whether it was a run', () => {
    const rows = flattenProjectHistory({
      s1: [
        { id: '1', timestamp: 2, kind: 'run', collection_path: 'a.httf', response: { status: 'ok' } },
        { id: '2', timestamp: 1, endpoint: 'GET /x', response: { status: 'ok' } },
      ],
    });
    expect(rows.map(r => r.kind)).toEqual(['run', undefined]);
    expect(rows[0].endpoint).toBe('a.httf');
  });

  it('keeps what the run’s checks came to', () => {
    const [row] = flattenProjectHistory({
      s1: [{
        id: '1', timestamp: 1, kind: 'run', collection_path: 'a.httf',
        response: { status: 'error', assertions_passed: 1, assertions_total: 2 },
      }],
    });
    expect(row.checks).toEqual({ passed: 1, total: 2 });
  });

  it('says nothing about a call that checked nothing', () => {
    const [row] = flattenProjectHistory({
      s1: [{ id: '1', timestamp: 1, endpoint: 'GET /x', response: { status: 'ok', assertions_total: 0 } }],
    });
    expect(row.checks).toBeUndefined();
  });
});

describe('where a recorded call went', () => {
  it('travels with the call', () => {
    const rows = flattenProjectHistory({
      s1: [{
        id: 'a', timestamp: 2, endpoint: 'pkg.Svc/M',
        connection: { address: 'staging:8443', protocol: 'grpc-web', tls: true },
        response: { status: 'ok' },
      }],
    });
    expect(rows[0].connection).toEqual({ address: 'staging:8443', protocol: 'grpc-web', tls: true });
  });

  it('is absent for a line that recorded none', () => {
    const rows = flattenProjectHistory({ s1: [{ id: 'a', timestamp: 1, endpoint: 'pkg.Svc/M', response: {} }] });
    expect(rows[0].connection).toBeUndefined();
  });
});

describe('what the line says about the call', () => {
  const one = (response: Record<string, unknown>) =>
    flattenProjectHistory({ s: [{ id: '1', timestamp: 1, endpoint: 'GET /x', response }] })[0];

  it('reads the code and the time the server wrote', () => {
    const row = one({ status: 'ok', status_code: 404, duration_ms: 12 });
    expect(row.response.statusCode).toBe(404);
    expect(row.response.durationMs).toBe(12);
  });

  it('leaves a line written before those were recorded alone', () => {
    const row = one({ status: 'ok' });
    expect(row.response.statusCode).toBe(null);
    expect(row.response.durationMs).toBe(null);
  });

  it('lets an HTTP failure be seen for what it is', () => {
    expect(entryFailed(one({ status: 'ok', status_code: 404 }))).toBe(true);
    expect(entryFailed(one({ status: 'ok', status_code: 200 }))).toBe(false);
  });
});

describe('the row a project line was made with', () => {
  it('carries it through', () => {
    const [entry] = flattenProjectHistory({
      s1: [{ id: 'a', timestamp: 1, endpoint: 'pkg.Svc/M', dataset_row: 1, bodies: ['{}'] }],
    });
    expect(entry.datasetRow).toBe(1);
  });

  it('says nothing for a file without rows', () => {
    const [entry] = flattenProjectHistory({
      s1: [{ id: 'a', timestamp: 1, endpoint: 'pkg.Svc/M', bodies: ['{}'] }],
    });
    expect(entry.datasetRow).toBeUndefined();
  });
});
