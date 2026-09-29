import { describe, expect, it } from 'vitest';
import { NO_TOGGLES, expandedDirs, toggleDir } from './rail-expansion';

const ROOTS = ['api', 'fixtures'];

describe('which folders of the rail are open', () => {
  it('opens the roots and the folders holding the open file', () => {
    expect([...expandedDirs(ROOTS, 'api/v1/users.gctf', NO_TOGGLES)]).toEqual(['api', 'fixtures', 'api/v1']);
  });

  it('keeps a folder the user closed closed, root or not', () => {
    const closedRoot = toggleDir(NO_TOGGLES, null, 'api', true);
    expect([...expandedDirs(ROOTS, null, closedRoot)]).toEqual(['fixtures']);
    expect([...expandedDirs(['api', 'fixtures', 'extra'], null, closedRoot)]).toEqual(['fixtures', 'extra']);
  });

  it('opens a closed folder again once a file inside it is chosen', () => {
    const closed = toggleDir(NO_TOGGLES, 'api/v1/users.gctf', 'api', true);
    expect(expandedDirs(ROOTS, 'api/v1/users.gctf', closed).has('api')).toBe(false);
    expect(expandedDirs(ROOTS, 'api/v2/orders.gctf', closed).has('api')).toBe(true);
  });

  it('lets the user close the folder of the chosen file and keeps it that way', () => {
    const closed = toggleDir(NO_TOGGLES, 'api/v1/users.gctf', 'api/v1', true);
    expect(expandedDirs(ROOTS, 'api/v1/users.gctf', closed).has('api/v1')).toBe(false);
    const reopened = toggleDir(closed, 'api/v1/users.gctf', 'api/v1', false);
    expect(expandedDirs(ROOTS, 'api/v1/users.gctf', reopened).has('api/v1')).toBe(true);
  });

  it('opens a folder no rule opens when the user asks', () => {
    const opened = toggleDir(NO_TOGGLES, null, 'api/v3', false);
    expect(expandedDirs(ROOTS, null, opened).has('api/v3')).toBe(true);
  });
});
