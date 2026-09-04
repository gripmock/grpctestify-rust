import { describe, expect, it } from 'vitest';
import { runtimeRow, transportDrift } from './plan-runtime';

describe('where a runtime value came from', () => {
  it('keeps a default quiet', () => {
    expect(runtimeRow({ key: 'retry', value: '0', source: 'CLI default' })).toEqual({
      key: 'retry', value: '0', from: 'default', fromFile: false,
    });
  });

  it('marks the row the file decided', () => {
    const row = runtimeRow({ key: 'timeout', value: '7 s', source: 'OPTIONS' });
    expect(row.from).toBe('set in OPTIONS');
    expect(row.fromFile).toBe(true);
  });

  it('says when a section attribute outranked the file', () => {
    const row = runtimeRow({ key: 'timeout', value: '5 s', source: 'attribute' });
    expect(row.from).toBe('set on the section');
    expect(row.fromFile).toBe(true);
  });

  it('passes an unknown layer through as it came', () => {
    const row = runtimeRow({ key: 'timeout', value: '9 s', source: 'the environment' });
    expect(row.from).toBe('the environment');
    expect(row.fromFile).toBe(true);
  });
});

describe('the transport the file does not name', () => {
  const rows = (protocol: string, source: string) =>
    [{ key: 'timeout', value: '30 s', source: 'CLI default' }, { key: 'protocol', value: protocol, source }]
      .map(runtimeRow);

  it('reports a workbench transport the file is silent about', () => {
    expect(transportDrift(rows('grpc', 'CLI default'), 'grpc-web')).toEqual({ chosen: 'grpc-web', file: 'grpc' });
  });

  it('says nothing when the workbench agrees with the file', () => {
    expect(transportDrift(rows('grpc', 'CLI default'), 'grpc')).toBeNull();
  });

  it('says nothing when the file names one', () => {
    expect(transportDrift(rows('connect', 'OPTIONS'), 'grpc-web')).toBeNull();
  });

  it('is answered by an OPTIONS the forms already hold', () => {
    expect(transportDrift(rows('grpc', 'CLI default'), 'grpc-web', 'grpc-web')).toBeNull();
    expect(transportDrift(rows('grpc', 'CLI default'), 'grpc-web', 'connectrpc'))
      .toEqual({ chosen: 'grpc-web', file: 'grpc' });
  });

  it('says nothing when there is no protocol row', () => {
    expect(transportDrift([runtimeRow({ key: 'timeout', value: '30 s', source: 'CLI default' })], 'grpc-web')).toBeNull();
  });
});
