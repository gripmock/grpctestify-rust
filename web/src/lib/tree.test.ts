import { describe, it, expect } from 'vitest';
import type { TreeNode } from './types';
import { NO_TAGS, activeFilters, benchRefusal, benchTakes, ancestorsOf, buildTree, childFolders, crumbs, failureGroups, failureReason, railSaysWhy, reasonsSaidOnce, familyOf, fileCount, filterByVerdict, filterTree, fixtureRole, renameNote, runTargets, saveExtFor, saveFolders, sortTree, withFamilyExt } from './tree';
import type { TagFilter } from './tree';

const tree = buildTree([
  { path: 'auth/login.gctf', name: 'login.gctf', is_dir: false },
  { path: 'auth/deep/refresh.gctf', name: 'refresh.gctf', is_dir: false },
  { path: 'empty', name: 'empty', is_dir: true },
]);

describe('fileCount', () => {
  it('counts files at any depth', () => {
    expect(fileCount(tree.find(n => n.name === 'auth')!)).toBe(2);
  });

  it('is zero for an empty folder', () => {
    expect(fileCount(tree.find(n => n.name === 'empty')!)).toBe(0);
  });

  it('is one for a file', () => {
    const auth = tree.find(n => n.name === 'auth')!;
    expect(fileCount(auth.children.find(c => !c.isDir)!)).toBe(1);
  });
});

describe('filterTree', () => {
  const nodes = [
    { name: 'auth', path: 'auth', isDir: true, children: [
      { name: 'login.gctf', path: 'auth/login.gctf', isDir: false, children: [], tags: ['smoke'] },
      { name: 'logout.gctf', path: 'auth/logout.gctf', isDir: false, children: [], tags: ['slow'] },
    ] },
  ];

  it('matches a tag as well as a file name', () => {
    const byName = filterTree(nodes, 'login', NO_TAGS);
    expect(byName[0].children.map(c => c.name)).toEqual(['login.gctf']);

    const byTag = filterTree(nodes, 'smoke', NO_TAGS);
    expect(byTag[0].children.map(c => c.name)).toEqual(['login.gctf']);
  });

  it('drops a folder whose files all filtered out', () => {
    expect(filterTree(nodes, 'nothing', NO_TAGS)).toEqual([]);
  });
});

describe('filterByVerdict', () => {
  const nodes = [
    { name: 'auth', path: 'auth', isDir: true, children: [
      { name: 'login.gctf', path: 'auth/login.gctf', isDir: false, children: [] },
      { name: 'logout.gctf', path: 'auth/logout.gctf', isDir: false, children: [] },
    ] },
    { name: 'feed', path: 'feed', isDir: true, children: [
      { name: 'crud.gctf', path: 'feed/crud.gctf', isDir: false, children: [] },
    ] },
  ];
  const verdicts = {
    'auth/login.gctf': { state: 'pass' },
    'auth/logout.gctf': { state: 'fail' },
    'feed/crud.gctf': { state: 'pass' },
  };

  it('keeps only the matching files and the folders that still hold one', () => {
    const failed = filterByVerdict(nodes, verdicts, 'fail');
    expect(failed.map(n => n.path)).toEqual(['auth']);
    expect(failed[0].children.map(c => c.path)).toEqual(['auth/logout.gctf']);
  });

  it('is the identity for all', () => {
    expect(filterByVerdict(nodes, verdicts, 'all')).toBe(nodes);
  });

  it('drops files a run never reached', () => {
    expect(filterByVerdict(nodes, {}, 'pass')).toEqual([]);
  });
});

describe('tag filtering matches the run flags', () => {
  const tagged = buildTree([
    { path: 'a/one.gctf', name: 'one', is_dir: false, tags: ['smoke', 'auth'] },
    { path: 'a/two.gctf', name: 'two', is_dir: false, tags: ['smoke'] },
    { path: 'a/three.gctf', name: 'three', is_dir: false, tags: ['smoke', 'flaky'] },
  ]);
  const names = (t: ReturnType<typeof buildTree>) =>
    t.flatMap(n => (n.isDir ? n.children : [n])).map(n => n.name);

  it('wants every included tag, not any of them', () => {
    expect(names(filterTree(tagged, '', { include: new Set(['smoke', 'auth']), exclude: new Set() })))
      .toEqual(['one.gctf']);
  });

  it('refuses a file carrying an excluded tag', () => {
    expect(names(filterTree(tagged, '', { include: new Set(['smoke']), exclude: new Set(['flaky']) })))
      .toEqual(['one.gctf', 'two.gctf']);
  });

  it('leaves everything alone with no tags at all', () => {
    expect(names(filterTree(tagged, '', NO_TAGS))).toHaveLength(3);
  });
});

describe('saveFolders', () => {
  const dir = (path: string, files: number): TreeNode => ({
    name: path.split('/').pop() ?? path,
    path,
    isDir: true,
    children: Array.from({ length: files }, (_, i) => ({
      name: `t${i}.gctf`, path: `${path}/t${i}.gctf`, isDir: false, children: [],
    })),
  });

  const dirs = [
    dir('crates', 0),
    dir('crates/apif-ast', 0),
    dir('crates/apif-ast/src', 0),
    dir('examples', 0),
    dir('examples/basic', 3),
    dir('tests', 1),
  ];

  it('offers where tests live, and the parents that lead there', () => {
    expect(saveFolders(dirs, '').map(d => d.path)).toEqual(['examples', 'examples/basic', 'tests']);
  });

  it('offers anything typed, empty folders included', () => {
    expect(saveFolders(dirs, 'apif-ast').map(d => d.path)).toEqual(['crates/apif-ast', 'crates/apif-ast/src']);
  });

  it('matches on the name as well as the path', () => {
    expect(saveFolders(dirs, 'SRC').map(d => d.path)).toEqual(['crates/apif-ast/src']);
  });
});

describe('activeFilters', () => {
  const filter = (include: string[], exclude: string[]): TagFilter =>
    ({ include: new Set(include), exclude: new Set(exclude) });

  it('is empty when nothing is filtering', () => {
    expect(activeFilters(filter([], []), false)).toEqual([]);
  });

  it('shows what is required, what is refused, and the changed selection', () => {
    expect(activeFilters(filter(['smoke'], ['slow']), true).map(f => f.label))
      .toEqual(['+smoke', '−slow', 'only changed']);
  });

  it('says what the changed selection is measured against', () => {
    expect(activeFilters(filter([], []), true, 'HEAD').map(f => f.label)).toEqual(['changed since HEAD']);
    expect(activeFilters(filter([], []), true, 'origin/main').map(f => f.label))
      .toEqual(['changed since origin/main']);
    expect(activeFilters(filter([], []), true, null).map(f => f.label)).toEqual(['only changed']);
  });

  it('keeps a stable order whatever order the tags were clicked in', () => {
    expect(activeFilters(filter(['users', 'auth'], []), false).map(f => f.label))
      .toEqual(['+auth', '+users']);
  });
});

describe('walking folders', () => {
  const dir = (path: string): TreeNode => ({
    name: path.split('/').pop() ?? path, path, isDir: true, children: [],
  });
  const dirs = [dir('examples'), dir('examples/basic'), dir('examples/advanced'), dir('tests'), dir('tests/gctf')];

  it('lists what is directly inside the root', () => {
    expect(childFolders(dirs, '').map(d => d.path)).toEqual(['examples', 'tests']);
  });

  it('lists what is directly inside a folder, and nothing deeper', () => {
    expect(childFolders(dirs, 'examples').map(d => d.path)).toEqual(['examples/basic', 'examples/advanced']);
    expect(childFolders(dirs, 'examples/basic')).toEqual([]);
  });

  it('does not mistake a prefix for a parent', () => {
    expect(childFolders([dir('test'), dir('tests/gctf')], 'test')).toEqual([]);
  });

  it('gives the way back up', () => {
    expect(crumbs('tests/gctf/streaming')).toEqual([
      { name: 'project root', path: '' },
      { name: 'tests', path: 'tests' },
      { name: 'gctf', path: 'tests/gctf' },
      { name: 'streaming', path: 'tests/gctf/streaming' },
    ]);
    expect(crumbs('')).toEqual([{ name: 'project root', path: '' }]);
  });
});

describe('familyOf', () => {
  it('tells the families apart by the only thing that says so', () => {
    expect(familyOf('login.gctf')).toBe('gctf');
    expect(familyOf('login.httf')).toBe('httf');
    expect(familyOf('checkout.apif')).toBe('apif');
  });

  it('calls anything else unknown rather than guessing', () => {
    expect(familyOf('README.md')).toBe('unknown');
    expect(familyOf('gctf')).toBe('unknown');
    expect(familyOf('.gctf')).toBe('gctf');
  });
});

describe('the extension a new file gets', () => {
  it('keeps the one that was typed', () => {
    expect(withFamilyExt('login.gctf')).toBe('login.gctf');
    expect(withFamilyExt('login.httf')).toBe('login.httf');
  });

  it('adds the family asked for when the name says nothing', () => {
    expect(withFamilyExt('login')).toBe('login.gctf');
    expect(withFamilyExt('login', 'httf')).toBe('login.httf');
  });

  it('does not take a dot in a name for an extension', () => {
    expect(withFamilyExt('v1.login')).toBe('v1.login.gctf');
  });
});

describe('filtering by what a file is called', () => {
  const tree = () => buildTree([
    { path: 'auth/login.gctf', name: 'login', is_dir: false, tags: [] },
    { path: 'billing/charge.gctf', name: 'charge', is_dir: false, tags: ['smoke'] },
  ]);

  it('reads the folder as part of the name', () => {
    const kept = filterTree(tree(), 'auth', NO_TAGS);
    expect(kept.flatMap(n => n.children).map(n => n.path)).toEqual(['auth/login.gctf']);
  });

  it('still reads the file and its tags', () => {
    expect(filterTree(tree(), 'charge', NO_TAGS).flatMap(n => n.children).map(n => n.path))
      .toEqual(['billing/charge.gctf']);
    expect(filterTree(tree(), 'smoke', NO_TAGS).flatMap(n => n.children).map(n => n.path))
      .toEqual(['billing/charge.gctf']);
  });
});

describe('the folders a file sits in', () => {
  it('are the path without the file, outermost first', () => {
    expect(ancestorsOf('users/admin/create.gctf')).toEqual(['users', 'users/admin']);
    expect(ancestorsOf('probe.httf')).toEqual([]);
  });
});

describe('the chips a filtered rail shows', () => {
  const none = { include: new Set<string>(), exclude: new Set<string>() };

  it('names the check filter when it is on', () => {
    expect(activeFilters(none, false, null, true))
      .toEqual([{ key: 'problems', label: 'with problems', kind: 'problems' }]);
  });

  it('says nothing about it when it is off', () => {
    expect(activeFilters(none, false, null, false)).toEqual([]);
  });

  it('keeps them in the order they were added', () => {
    expect(activeFilters({ include: new Set(['a']), exclude: new Set() }, true, 'main', true).map(c => c.key))
      .toEqual(['+a', 'changed', 'problems']);
  });
});

describe('what a rail row runs', () => {
  const file = (path: string): TreeNode => ({ name: path.split('/').pop()!, path, isDir: false, children: [] });
  const dir = (path: string, children: TreeNode[]): TreeNode =>
    ({ name: path.split('/').pop()!, path, isDir: true, children });

  it('runs the one file a file row points at', () => {
    const node = file('auth/login.gctf');
    expect(runTargets(node, ['auth/login.gctf', 'auth/logout.gctf'])).toEqual(['auth/login.gctf']);
  });

  it('runs everything under a folder, however deep', () => {
    const node = dir('catalog', [
      file('catalog/list.gctf'),
      dir('catalog/items', [file('catalog/items/get.gctf')]),
    ]);
    expect(runTargets(node, ['catalog/list.gctf', 'catalog/items/get.gctf', 'auth/login.gctf']))
      .toEqual(['catalog/list.gctf', 'catalog/items/get.gctf']);
  });

  it('runs only what the rail is showing', () => {
    const node = dir('catalog', [file('catalog/a.gctf'), file('catalog/b.gctf')]);
    expect(runTargets(node, ['catalog/b.gctf'])).toEqual(['catalog/b.gctf']);
  });

  it('has nothing to run in an empty folder', () => {
    expect(runTargets(dir('empty', []), ['a.gctf'])).toEqual([]);
  });

  it('runs a folder with the fixtures of the folder', () => {
    const node = dir('api', [
      file('api/_setup.httf'),
      file('api/list.httf'),
      file('api/_teardown.httf'),
    ]);
    const all = ['api/_setup.httf', 'api/list.httf', 'api/_teardown.httf'];
    expect(runTargets(node, ['api/list.httf'], all))
      .toEqual(['api/list.httf', 'api/_setup.httf', 'api/_teardown.httf']);
  });

  it('leaves the fixtures of a folder none of whose tests are showing', () => {
    const node = dir('api', [
      dir('api/v1', [file('api/v1/_setup.httf'), file('api/v1/list.httf')]),
      dir('api/v2', [file('api/v2/_setup.httf'), file('api/v2/list.httf')]),
    ]);
    const all = ['api/v1/_setup.httf', 'api/v1/list.httf', 'api/v2/_setup.httf', 'api/v2/list.httf'];
    expect(runTargets(node, ['api/v1/list.httf'], all))
      .toEqual(['api/v1/list.httf', 'api/v1/_setup.httf']);
  });

  it('runs one file without the fixtures beside it', () => {
    const node = file('api/list.httf');
    expect(runTargets(node, ['api/list.httf'], ['api/_setup.httf', 'api/list.httf']))
      .toEqual(['api/list.httf']);
  });
});

describe('what a file is to its folder', () => {
  it('reads the convention by name, in either family', () => {
    expect(fixtureRole('api/_setup.httf')).toBe('setup');
    expect(fixtureRole('_setup.gctf')).toBe('setup');
    expect(fixtureRole('api/_teardown.gctf')).toBe('teardown');
    expect(fixtureRole('api/list.httf')).toBeNull();
    expect(fixtureRole('api/_setup.md')).toBeNull();
  });

  it('puts the setup first and the teardown last', () => {
    const nodes = buildTree([
      { path: 'api/list.httf', name: 'list.httf', is_dir: false },
      { path: 'api/_teardown.httf', name: '_teardown.httf', is_dir: false },
      { path: 'api/create.httf', name: 'create.httf', is_dir: false },
      { path: 'api/_setup.httf', name: '_setup.httf', is_dir: false },
    ]);
    expect(sortTree(nodes)[0].children.map(n => n.name))
      .toEqual(['_setup.httf', 'create.httf', 'list.httf', '_teardown.httf']);
  });
});

describe('why a red run was red', () => {
  const verdicts = {
    'auth/login.gctf': { state: 'fail', message: 'Validation error: At least one verification section (RESPONSE, ERROR, or ASSERTS) is required', path: 'auth/login.gctf' },
    'auth/logout.gctf': { state: 'fail', message: 'Validation error: At least one verification section (RESPONSE, ERROR, or ASSERTS) is required', path: 'auth/logout.gctf' },
    'health/check.gctf': { state: 'fail', message: 'Could not reach localhost:4770: Connection refused', path: 'health/check.gctf' },
    'health/watch.gctf': { state: 'pass', message: '', path: 'health/watch.gctf' },
  };

  it('counts the reasons, most files first', () => {
    const groups = failureGroups(verdicts);
    expect(groups.map(g => g.paths.length)).toEqual([2, 1]);
    expect(groups[0].reason).toContain('At least one verification section');
    expect(groups[0].paths).toEqual(['auth/login.gctf', 'auth/logout.gctf']);
    expect(groups[1].reason).toContain('Could not reach localhost:4770');
  });

  it('says nothing about the files that passed', () => {
    expect(failureGroups(verdicts).flatMap(g => g.paths)).not.toContain('health/watch.gctf');
  });

  it('reads two files that failed the same way as one reason', () => {
    const one = failureReason('Validation failed:\n  - Assertion failed at line 33 (assertion at line 34): Assertion failed: .ok == true (Values: null vs true)');
    const two = failureReason('Validation failed:\n  - Assertion failed at line 8 (assertion at line 9): Assertion failed: .ok == true (Values: 3 vs true)');
    expect(one).toBe(two);
    expect(one).toBe('Assertion failed: .ok == true');
  });

  it('skips the heading and reads the reason', () => {
    expect(failureReason('Validation error: Validation failed:\nAt least one verification section (RESPONSE, ERROR, or ASSERTS) is required'))
      .toBe('At least one verification section (RESPONSE, ERROR, or ASSERTS) is required');
    expect(failureReason('Validation failed:\n  - Expected message for RESPONSE section at line 21, but received Trailers (End of Stream)'))
      .toBe('Expected message for RESPONSE section, but received Trailers (End of Stream)');
  });

  it('keeps a message that is one line of its own', () => {
    expect(failureReason('Could not reach localhost:4770: Connection refused'))
      .toBe('Could not reach localhost:4770: Connection refused');
    expect(failureReason(undefined)).toBe('');
  });
});

describe('the rail narrowed to one reason', () => {
  const nodes = buildTree([
    { path: 'auth/login.gctf', name: 'login.gctf', is_dir: false },
    { path: 'auth/logout.gctf', name: 'logout.gctf', is_dir: false },
    { path: 'health/check.gctf', name: 'check.gctf', is_dir: false },
  ]);
  const verdicts = {
    'auth/login.gctf': { state: 'fail', message: 'no verification section' },
    'auth/logout.gctf': { state: 'fail', message: 'Could not reach localhost:4770' },
    'health/check.gctf': { state: 'pass', message: '' },
  };

  it('keeps the files that failed that way and no others', () => {
    const shown = filterByVerdict(nodes, verdicts, 'all', 'no verification section');
    expect(shown.flatMap(d => d.children.map(c => c.path))).toEqual(['auth/login.gctf']);
  });

  it('is the whole rail again with no reason chosen', () => {
    expect(filterByVerdict(nodes, verdicts, 'all', null)).toEqual(nodes);
  });
});

describe('the extension a bare name gets', () => {
  it('is the open file’s own family', () => {
    expect(saveExtFor('a/checkout.apif')).toBe('apif');
    expect(saveExtFor('a/list.httf')).toBe('httf');
    expect(saveExtFor('a/login.gctf')).toBe('gctf');
  });

  it('is gctf where there is no open file to read', () => {
    expect(saveExtFor(null)).toBe('gctf');
    expect(saveExtFor('notes.md')).toBe('gctf');
  });

  it('leaves a name that already carries one', () => {
    expect(withFamilyExt('checkout.apif', 'gctf')).toBe('checkout.apif');
    expect(withFamilyExt('checkout', 'apif')).toBe('checkout.apif');
  });
});

describe('renameNote', () => {
  it('says nothing about a move inside one family', () => {
    expect(renameNote('auth/login.gctf', 'smoke/login.gctf')).toBeNull();
    expect(renameNote('a.httf', 'b.httf')).toBeNull();
  });

  it('names the grammar a new family reads by', () => {
    const note = renameNote('login.gctf', 'login.httf');
    expect(note).toContain('HTTP test');
    expect(note).toContain('ENDPOINT');
  });

  it('says a file that stops being a test leaves the collection', () => {
    expect(renameNote('login.gctf', 'login.txt')).toContain('no run, check or bench picks it up');
  });

  it('says a file that becomes one will now be run', () => {
    expect(renameNote('notes.txt', 'notes.apif')).toContain('a run will pick it up');
  });
});

describe('the reasons already said once', () => {
  const same = 'Validation error: At least one verification section (RESPONSE, ERROR, or ASSERTS) is required';
  const verdicts = {
    'a.gctf': { state: 'fail', message: same, path: 'a.gctf' },
    'b.gctf': { state: 'fail', message: same, path: 'b.gctf' },
    'c.gctf': { state: 'fail', message: 'Could not reach localhost:4770: Connection refused', path: 'c.gctf' },
  };

  it('holds a reason more than one file failed for', () => {
    expect(reasonsSaidOnce(verdicts).has(failureReason(same))).toBe(true);
  });

  it('leaves a reason of its own alone', () => {
    expect(reasonsSaidOnce(verdicts).has(failureReason('Could not reach localhost:4770: Connection refused'))).toBe(false);
  });

  it('stops at the three the bar states', () => {
    const many: Record<string, { state: string; message: string; path: string }> = {};
    for (const [i, reason] of ['one', 'two', 'three', 'four'].entries()) {
      many[`${reason}-1.gctf`] = { state: 'fail', message: `Reason ${reason}`, path: `${reason}-1.gctf` };
      many[`${reason}-2.gctf`] = { state: 'fail', message: `Reason ${reason}`, path: `${reason}-2.gctf` };
      if (i === 3) many[`${reason}-3.gctf`] = { state: 'fail', message: `Reason ${reason}`, path: `${reason}-3.gctf` };
    }
    const said = reasonsSaidOnce(many);
    expect(said.size).toBe(3);
    expect(said.has('Reason four')).toBe(true);
  });

  it('says nothing at all about a run with no repeated reason', () => {
    expect(reasonsSaidOnce({ 'c.gctf': verdicts['c.gctf'] }).size).toBe(0);
  });
});

describe('whether a failing row spells its reason out', () => {
  const same = 'Validation error: At least one verification section is required';
  const said = new Set([failureReason(same)]);

  it('leaves the shared reason to the bar', () => {
    expect(railSaysWhy(same, said, false)).toBe(false);
  });

  it('says a reason of its own', () => {
    expect(railSaysWhy('Could not reach localhost:4770', said, false)).toBe(true);
  });

  it('keeps the words on a row that offers the fix', () => {
    expect(railSaysWhy(same, said, true)).toBe(true);
  });

  it('says everything when the bar states nothing', () => {
    expect(railSaysWhy(same, new Set(), false)).toBe(true);
  });
});

describe('what the load runner takes', () => {
  it('takes a .gctf', () => {
    expect(benchTakes('auth/login.gctf')).toBe(true);
    expect(benchRefusal('auth/login.gctf')).toBe(null);
  });

  it('does not take an .httf, and says it has no gRPC call', () => {
    expect(benchTakes('probe.httf')).toBe(false);
    expect(benchRefusal('probe.httf')).toContain('an .httf has none');
  });

  it('does not take an .apif, and says why in its own words', () => {
    expect(benchTakes('checkout.apif')).toBe(false);
    expect(benchRefusal('checkout.apif')).toContain('.apif holds steps of both transports');
  });

  it('says something about a file of no family at all', () => {
    expect(benchTakes('notes.txt')).toBe(false);
    expect(benchRefusal('notes.txt')).toContain('not a .gctf');
  });
});
