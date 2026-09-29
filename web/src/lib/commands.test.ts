import { describe, expect, it } from 'vitest';
import { COMMANDS, SAY_TOAST, commandRefusal, filterCommands, matchesCommand } from './commands';
import { REFUSAL_TYPE, toastLife } from 'luvo/ui/toast-life';
import type { PlayStore } from './types';

const state = {
  workspacePath: null, runJobId: null, tabs: [], activeTabId: null, request: { endpoint: '' },
  visibleFiles: [], collectionParsed: null, benchBaseline: null, run: { benchReport: null },
} as unknown as PlayStore;

const benchable = {
  ...state,
  workspacePath: 'load.gctf',
  visibleFiles: ['load.gctf', 'other.gctf'],
  collectionParsed: { bench: { mode: 'fixed' } },
  benchBaseline: { summary: {} },
  run: { benchReport: { summary: {} } },
} as unknown as PlayStore;

describe('matchesCommand', () => {
  it('matches a subsequence, not just a prefix', () => {
    expect(matchesCommand('Run this file', 'rtf')).toBe(true);
    expect(matchesCommand('Run this file', 'file')).toBe(true);
    expect(matchesCommand('Run this file', 'zz')).toBe(false);
  });

  it('treats an empty query as everything', () => {
    expect(matchesCommand('Anything', '  ')).toBe(true);
  });
});

describe('filterCommands', () => {
  it('hides what the current state cannot do', () => {
    const ids = filterCommands(COMMANDS, '', state).map(c => c.id);
    expect(ids).not.toContain('run.file');
    expect(ids).not.toContain('run.cancel');
    expect(ids).toContain('run.all');
  });

  it('offers a bench only where one can be run', () => {
    const idle = filterCommands(COMMANDS, '', state).map(c => c.id);
    expect(idle).not.toContain('bench.file');
    expect(idle).not.toContain('bench.visible');
    expect(idle).not.toContain('bench.compare');

    const ready = filterCommands(COMMANDS, '', benchable).map(c => c.id);
    expect(ready).toContain('bench.file');
    expect(ready).toContain('bench.visible');
    expect(ready).toContain('bench.compare');
  });

  it('refuses to bench a file that carries no BENCH section', () => {
    const noSection = { ...benchable, collectionParsed: { bench: {} } } as unknown as PlayStore;
    expect(filterCommands(COMMANDS, '', noSection).map(c => c.id)).not.toContain('bench.file');
  });
});

describe('COMMANDS', () => {
  it('gives Execute and Run a key each', () => {
    const key = (id: string) => COMMANDS.find(c => c.id === id)?.hotkey;
    expect(key('execute')).toEqual({ key: 'Enter', ctrl: true });
    expect(key('run.file')).toEqual({ key: 'Enter', ctrl: true, shift: true });
  });

  it('offers check by name and by key', () => {
    const ids = COMMANDS.map(c => c.id);
    expect(ids).toContain('check.scope');
    expect(ids).toContain('check.file');
    expect(ids).toContain('check.all');
    expect(COMMANDS.find(c => c.id === 'check.scope')?.hotkey).toEqual({ key: 'k', ctrl: true, shift: true });
  });

  it('has no duplicate ids and no duplicate hotkeys', () => {
    const ids = COMMANDS.map(c => c.id);
    expect(new Set(ids).size).toBe(ids.length);
    const keys = COMMANDS.filter(c => c.hotkey).map(c =>
      [c.hotkey!.key, c.hotkey!.ctrl, c.hotkey!.shift, c.hotkey!.alt].join('|'));
    expect(new Set(keys).size).toBe(keys.length);
  });
});

const RELOCATED = [
  'connection.address',
  'file.saveAs',
  'request.import',
  'view.layout',
  'view.theme',
  'view.palette',
  'run.file',
  'run.folder',
  'run.all',
  'run.cancel',
  'collections.refresh',
];

describe('relocated capabilities', () => {
  it('all still have a command', () => {
    const ids = new Set(COMMANDS.map(c => c.id));
    for (const id of RELOCATED) expect(ids.has(id)).toBe(true);
  });
});

describe('closing a tab', () => {
  it('raises an intent instead of removing the tab behind the prompt', () => {
    const cmd = COMMANDS.find(c => c.id === 'tab.close')!;
    const calls: string[] = [];
    const st = {
      activeTabId: 't1',
      requestCloseTab: () => calls.push('requestCloseTab'),
      removeTab: () => calls.push('removeTab'),
    } as unknown as PlayStore;
    cmd.run(st, {} as never);
    expect(calls).toEqual(['requestCloseTab']);
  });
});

describe('the files a run command reaches', () => {
  it('is both families', () => {
    let ran: string[] = [];
    const withFiles = {
      ...state,
      collections: [
        { path: 'a/users.gctf', is_dir: false },
        { path: 'a/health.httf', is_dir: false },
        { path: 'a', is_dir: true },
        { path: 'a/notes.md', is_dir: false },
      ],
      startRun: (paths: string[]) => { ran = paths; },
    } as unknown as PlayStore;

    COMMANDS.find(c => c.id === 'run.all')!.run(withFiles, {} as never);
    expect(ran).toEqual(['a/users.gctf', 'a/health.httf']);
  });
});

describe('what a run from a command covers', () => {
  const project = [
    { path: 'a/users.gctf', is_dir: false },
    { path: 'a/health.gctf', is_dir: false },
    { path: 'b/other.gctf', is_dir: false },
  ];

  it('is what the rail is showing, not every file in the project', () => {
    let ran: string[] = [];
    const s = {
      ...state,
      collections: project,
      visibleFiles: ['a/users.gctf'],
      workspacePath: 'a/users.gctf',
      runScope: 'all',
      startRun: (paths: string[]) => { ran = paths; },
    } as unknown as PlayStore;

    COMMANDS.find(c => c.id === 'run.all')!.run(s, {} as never);
    expect(ran).toEqual(['a/users.gctf']);
    COMMANDS.find(c => c.id === 'run.scope')!.run(s, {} as never);
    expect(ran).toEqual(['a/users.gctf']);
    COMMANDS.find(c => c.id === 'run.folder')!.run(s, {} as never);
    expect(ran).toEqual(['a/users.gctf']);
  });

  it('falls back to the project when no rail has reported one', () => {
    let ran: string[] = [];
    const s = {
      ...state,
      collections: project,
      visibleFiles: [],
      startRun: (paths: string[]) => { ran = paths; },
    } as unknown as PlayStore;

    COMMANDS.find(c => c.id === 'run.all')!.run(s, {} as never);
    expect(ran).toEqual(['a/users.gctf', 'a/health.gctf', 'b/other.gctf']);
  });
});

describe('what the load runner can measure', () => {
  it('does not offer to bench an HTTP file', () => {
    const s = {
      ...state,
      workspacePath: 'probe.httf',
      request: { endpoint: 'GET /v1/users' },
      collectionParsed: { bench: { mode: 'fixed' } },
      visibleFiles: ['probe.httf'],
    } as unknown as PlayStore;
    expect(COMMANDS.find(c => c.id === 'bench.file')?.enabled?.(s)).toBe(false);
    expect(COMMANDS.find(c => c.id === 'bench.visible')?.enabled?.(s)).toBe(false);
  });

  it('offers it for a gRPC one', () => {
    const s = {
      ...state,
      workspacePath: 'a.gctf',
      request: { endpoint: 'a.B/C' },
      collectionParsed: { bench: { mode: 'fixed' } },
      visibleFiles: ['a.gctf', 'probe.httf'],
    } as unknown as PlayStore;
    expect(COMMANDS.find(c => c.id === 'bench.file')?.enabled?.(s)).toBe(true);
    expect(COMMANDS.find(c => c.id === 'bench.visible')?.enabled?.(s)).toBe(true);
  });
});

describe('scaffolding a test', () => {
  it('is not offered for an HTTP request', () => {
    const s = { ...state, workspacePath: 'p.httf', request: { endpoint: 'GET /v1/users' } } as unknown as PlayStore;
    expect(COMMANDS.find(c => c.id === 'file.scaffold')?.enabled?.(s)).toBe(false);
  });

  it('is offered for a gRPC one', () => {
    const s = { ...state, workspacePath: 'a.gctf', request: { endpoint: 'a.B/C' } } as unknown as PlayStore;
    expect(COMMANDS.find(c => c.id === 'file.scaffold')?.enabled?.(s)).toBe(true);
  });
});

describe('what a key says when the command cannot run', () => {
  const find = (id: string) => COMMANDS.find(c => c.id === id)!;

  it('says nothing when the command can run', () => {
    const s = { ...state, request: { endpoint: 'a.B/C' } } as unknown as PlayStore;
    expect(commandRefusal(find('execute'), s)).toBeNull();
  });

  it('says what is missing', () => {
    expect(commandRefusal(find('execute'), state))
      .toBe('Name an endpoint first — there is nothing to send');
    expect(commandRefusal(find('run.file'), state))
      .toBe('A run reads a file from disk — save this tab first');
  });

  it('tells a call that never arrived from a call never made', () => {
    const dialled = {
      ...state,
      response: { status: 'error', statusCode: null, messages: [], error: 'Connection refused' },
    } as unknown as PlayStore;
    expect(commandRefusal(find('response.expect'), dialled))
      .toBe('The call never reached a server — there is no answer to expect');
    expect(commandRefusal(find('response.expect'), state))
      .toBe('Execute the request first — there is no answer to expect');
  });

  it('offers it once a server has answered', () => {
    const answered = {
      ...state,
      response: { status: 'ok', statusCode: 0, messages: [{ a: 1 }], error: null },
    } as unknown as PlayStore;
    expect(commandRefusal(find('response.expect'), answered)).toBeNull();
  });

  it('says nothing about a command that is always available', () => {
    expect(commandRefusal(find('file.save'), state)).toBeNull();
  });

  it('leaves no hotkey refusing in the generic sentence', () => {
    const generic = COMMANDS.filter(c => c.hotkey && c.enabled && !c.why).map(c => c.id);
    expect(generic).toEqual([]);
  });
});

describe('how a command speaks', () => {
  it('keeps a failure on screen and lets a refusal go', () => {
    expect(SAY_TOAST.note).toBe('refuse');
    expect(toastLife(REFUSAL_TYPE)).toBe(4000);
    expect(toastLife('error')).toBeNull();
    expect(toastLife('success')).toBe(4000);
  });
});
