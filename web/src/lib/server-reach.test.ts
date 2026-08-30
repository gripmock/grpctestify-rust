import { describe, it, expect } from 'vitest';
import { isNetworkFailure, noteReachable, noteUnreachable, serverReachable, subscribeReach } from './server-reach';

describe('whether the workbench is answering', () => {
  it('is said once, and taken back once', () => {
    let told = 0;
    const stop = subscribeReach(() => { told += 1; });
    expect(serverReachable()).toBe(true);

    noteUnreachable();
    noteUnreachable();
    expect(told).toBe(1);
    expect(serverReachable()).toBe(false);

    noteReachable();
    expect(told).toBe(2);
    expect(serverReachable()).toBe(true);
    stop();
  });

  it('is not what a cancelled request means', () => {
    expect(isNetworkFailure(new DOMException('aborted', 'AbortError'))).toBe(false);
    expect(isNetworkFailure(new TypeError('Failed to fetch'))).toBe(true);
  });
});
