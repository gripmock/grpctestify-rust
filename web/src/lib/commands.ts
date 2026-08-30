import type { HotkeyDef } from 'luvo/input/hotkeys';
import type { PlayStore } from './types';
import { scopeFiles } from './jobs';
import { copyToClipboard } from 'luvo/data/clipboard';
import { nextMode } from 'luvo/theme/themes';
import { familyOf } from './tree';
import { encodeCollectionLink } from './deeplink';
import { isHttpRequest } from './http-endpoint';
import { benchTakes } from './tree';
import { useStore, workspaceDirty } from './store';
import { copiedNote } from './duplicate-name';
import { count } from 'luvo/data/plural';
import { serverAnswered } from './answer-source';

export type CommandCategory = HotkeyDef['category'] | 'run' | 'file';

export interface Command {
  id: string;
  title: string;
  category: CommandCategory;
  hotkey?: Omit<HotkeyDef, 'category' | 'description'>;
  enabled?: (s: PlayStore) => boolean;
  why?: (s: PlayStore) => string;
  run: (s: PlayStore, ui: CommandUi) => void;
}

export interface CommandUi {
  openPalette: () => void;
  closePalette: () => void;
  openHelp: () => void;
  saveFile: () => void;
  openImport: () => void;
  say: (kind: 'ok' | 'bad' | 'note', message: string) => void;
}

const filesOf = (s: PlayStore) =>
  s.collections.filter(c => !c.is_dir && familyOf(c.path) !== 'unknown').map(c => c.path);

const railFiles = (s: PlayStore) => (s.visibleFiles.length > 0 ? s.visibleFiles : filesOf(s));

const hasBench = (s: PlayStore) => Object.keys(s.collectionParsed?.bench ?? {}).length > 0;
const benchableOf = (s: PlayStore) => s.visibleFiles.filter(f => familyOf(f) !== 'httf');

export const COMMANDS: Command[] = [
  {
    id: 'execute',
    title: 'Execute request',
    category: 'execution',
    hotkey: { key: 'Enter', ctrl: true },
    enabled: s => !!s.request.endpoint,
    why: () => 'Name an endpoint first — there is nothing to send',
    run: s => void s.execute(),
  },
  {
    id: 'run.scope',
    title: 'Run — the current scope',
    category: 'run',
    hotkey: { key: 'r', ctrl: true, shift: true },
    run: s => void s.startRun(scopeFiles(railFiles(s), s.runScope, s.workspacePath)),
  },
  {
    id: 'run.file',
    title: 'Run this file',
    category: 'run',
    hotkey: { key: 'Enter', ctrl: true, shift: true },
    enabled: s => !!s.workspacePath,
    why: () => 'A run reads a file from disk — save this tab first',
    run: s => void s.startRun(scopeFiles(railFiles(s), 'file', s.workspacePath)),
  },
  {
    id: 'run.folder',
    title: 'Run this folder',
    category: 'run',
    enabled: s => !!s.workspacePath,
    run: s => void s.startRun(scopeFiles(railFiles(s), 'folder', s.workspacePath)),
  },
  {
    id: 'run.all',
    title: 'Run everything the rail is showing',
    category: 'run',
    run: s => void s.startRun(railFiles(s)),
  },
  {
    id: 'bench.file',
    title: 'Bench this file',
    category: 'run',
    enabled: s => !!s.workspacePath && benchTakes(s.workspacePath) && hasBench(s),
    run: s => void s.startBench(s.workspacePath!),
  },
  {
    id: 'bench.visible',
    title: 'Bench every .gctf the rail is showing',
    category: 'run',
    enabled: s => benchableOf(s).length > 0,
    run: s => void s.startBench(benchableOf(s)),
  },
  {
    id: 'bench.compare',
    title: 'Compare this bench with the previous one',
    category: 'run',
    enabled: s => s.benchBaseline !== null && s.run.benchReport !== null,
    run: s => void s.compareBench(),
  },
  {
    id: 'check.scope',
    title: 'Check — the current scope',
    category: 'run',
    hotkey: { key: 'k', ctrl: true, shift: true },
    run: (s, ui) => void s
      .checkAll(scopeFiles(railFiles(s), s.runScope, s.workspacePath))
      .then(() => { const said = useStore.getState().checkedSaid; if (said) ui.say('ok', said); }),
  },
  {
    id: 'check.file',
    title: 'Check this file',
    category: 'run',
    enabled: s => !!s.workspacePath,
    run: (s, ui) => void s
      .checkAll(scopeFiles(railFiles(s), 'file', s.workspacePath))
      .then(() => { const said = useStore.getState().checkedSaid; if (said) ui.say('ok', said); }),
  },
  {
    id: 'check.all',
    title: 'Check everything the rail is showing',
    category: 'run',
    run: (s, ui) => void s
      .checkAll(railFiles(s))
      .then(() => { const said = useStore.getState().checkedSaid; if (said) ui.say('ok', said); }),
  },
  {
    id: 'run.cancel',
    title: 'Cancel the run',
    category: 'run',
    enabled: s => s.runJobId !== null,
    run: s => void s.cancelRun(),
  },
  {
    id: 'file.save',
    title: 'Save file',
    category: 'file',
    hotkey: { key: 's', ctrl: true },
    run: (_s, ui) => ui.saveFile(),
  },
  {
    id: 'file.scaffold',
    title: 'Scaffold a test for this method',
    category: 'file',
    enabled: s => s.request.endpoint !== '' && !isHttpRequest(s.workspacePath, s.request.endpoint),
    run: (s, ui) => void s.scaffoldTest()
      .then(() => ui.say('ok', 'Scaffold opened — Save to name it'))
      .catch(err => ui.say('bad', err?.message || 'Scaffold failed')),
  },
  {
    id: 'file.discard',
    title: 'Discard edits — read the file again',
    category: 'file',
    enabled: s => !!s.workspacePath && workspaceDirty(s),
    run: s => s.requestDiscard(),
  },
  {
    id: 'file.saveAs',
    title: 'Save file as…',
    category: 'file',
    hotkey: { key: 'S', ctrl: true, shift: true },
    run: s => s.requestSaveAs(),
  },
  {
    id: 'file.format',
    title: 'Format this file — the same `fmt` the CLI runs',
    category: 'file',
    enabled: s => !!s.workspacePath,
    run: (s, ui) => void s.formatFile()
      .then(lines => ui.say('ok', lines === 0
        ? 'Already formatted'
        : `Formatted — ${count(lines, 'line')} changed · save to keep it`))
      .catch(err => ui.say('bad', err?.message || 'The formatter refused this file')),
  },
  {
    id: 'file.duplicate',
    title: 'Duplicate this file',
    category: 'file',
    enabled: s => !!s.workspacePath,
    run: (s, ui) => void s.duplicateCollection(s.workspacePath!)
      .then(name => {
        if (!name) return;
        ui.say('ok', copiedNote(name, s.workspacePath!, workspaceDirty(s)));
      })
      .catch(err => ui.say('bad', err?.message || 'The file could not be copied')),
  },
  {
    id: 'tab.close',
    title: 'Close current tab',
    category: 'tabs',
    hotkey: { key: 'w', ctrl: true },
    enabled: s => s.activeTabId !== null,
    why: () => 'No tab is open',
    run: s => s.requestCloseTab(),
  },
  {
    id: 'tab.list',
    title: 'Show all open tabs',
    category: 'tabs',
    enabled: s => s.tabs.length > 1,
    run: s => s.requestTabList(),
  },
  {
    id: 'tab.closeAll',
    title: 'Close all tabs',
    category: 'tabs',
    enabled: s => s.tabs.length > 0,
    run: s => s.requestCloseAllTabs(),
  },
  {
    id: 'tab.new',
    title: 'New tab',
    category: 'tabs',
    hotkey: { key: 'T', ctrl: true, shift: true },
    run: (s, ui) => {
      if (!s.addTab()) ui.say('bad', 'Every open tab has unsaved edits — close one first');
    },
  },
  {
    id: 'tab.next',
    title: 'Next tab',
    category: 'tabs',
    hotkey: { key: ']', ctrl: true, shift: true },
    enabled: s => s.tabs.length > 1,
    why: () => 'Only one tab is open',
    run: s => stepTab(s, 1),
  },
  {
    id: 'tab.prev',
    title: 'Previous tab',
    category: 'tabs',
    hotkey: { key: '[', ctrl: true, shift: true },
    enabled: s => s.tabs.length > 1,
    why: () => 'Only one tab is open',
    run: s => stepTab(s, -1),
  },
  {
    id: 'view.sidebar',
    title: 'Toggle sidebar',
    category: 'navigation',
    hotkey: { key: 'b', ctrl: true },
    run: s => s.toggleSidebar(),
  },
  {
    id: 'view.tools',
    title: 'Toggle tools — jq, regex, schema',
    category: 'navigation',
    hotkey: { key: 'J', ctrl: true, shift: true },
    run: s => s.setDrawerOpen(!s.drawerOpen),
  },
  {
    id: 'view.layout',
    title: 'Switch layout — columns or rows',
    category: 'navigation',
    hotkey: { key: 'l', ctrl: true, alt: true },
    run: s => s.setLayout(s.layout === 'columns' ? 'rows' : 'columns'),
  },
  {
    id: 'view.theme',
    title: 'Light, dark or system',
    category: 'navigation',
    run: s => s.setMode(nextMode(s.mode)),
  },
  {
    id: 'view.palette',
    title: 'Command palette',
    category: 'general',
    hotkey: { key: 'k', ctrl: true },
    run: (_s, ui) => ui.openPalette(),
  },
  {
    id: 'view.help',
    title: 'Keyboard shortcuts help',
    category: 'general',
    hotkey: { key: '?' },
    run: (_s, ui) => ui.openHelp(),
  },
  {
    id: 'run.check',
    title: 'Check every file the rail is showing',
    category: 'run',
    run: (s, ui) => {
      const files = railFiles(s);
      if (files.length === 0) { ui.say('bad', 'The rail is showing no files'); return; }
      void s.checkAll(files).then(() => {
        const said = useStore.getState().checkedSaid;
        if (said) ui.say(said.includes('nothing to report') ? 'ok' : 'bad', said);
      });
    },
  },
  {
    id: 'request.import',
    title: 'Import a curl or grpcurl command…',
    category: 'file',
    run: (_s, ui) => ui.openImport(),
  },
  {
    id: 'response.expect',
    title: 'Expect this — write the expectation from the answer',
    category: 'file',
    enabled: s => serverAnswered(s.response),
    why: s => (s.response && s.response.status !== 'pending'
      ? 'The call never reached a server — there is no answer to expect'
      : 'Execute the request first — there is no answer to expect'),
    run: (s, ui) => {
      if (s.expectFromResponse()) ui.say('ok', 'Expectation written from this answer');
      else ui.say('bad', "This answer is another step's — save or discard this step's edits first");
    },
  },
  {
    id: 'request.grpcurl',
    title: 'Copy this request as a curl or grpcurl command',
    category: 'file',
    enabled: s => (s.request?.endpoint ?? '').trim() !== '',
    run: (s, ui) => {
      const http = isHttpRequest(s.workspacePath, s.request?.endpoint ?? '');
      const line = http
        ? Promise.resolve(s.getCurlCommand())
        : s.getGrpcurlCommand();
      const what = http ? 'curl' : 'grpcurl';
      void line.then(
        command => copyToClipboard(command).then(
          () => ui.say('ok', `${what} command copied`),
          () => ui.say('bad', 'The browser refused the clipboard'),
        ),
        () => ui.say('bad', 'The command could not be built'),
      );
    },
  },
  {
    id: 'request.share',
    title: 'Share this request…',
    category: 'file',
    run: (s, ui) => {
      const kind = s.startShare();
      if (kind !== 'link') return;
      const path = s.tabs.find(t => t.id === s.activeTabId)?.collectionPath;
      if (!path) return;
      void copyToClipboard(`${window.location.origin}${encodeCollectionLink(path)}`)
        .then(() => ui.say('ok', 'Collection link copied'))
        .catch(() => ui.say('bad', 'The browser refused the clipboard'));
    },
  },
  {
    id: 'env.manage',
    title: 'Environments…',
    category: 'general',
    run: s => s.openEnvManager(),
  },
  {
    id: 'docs.preview',
    title: 'API docs for what the rail shows',
    category: 'run',
    run: s => s.setDocsOpen(true),
  },
  {
    id: 'connection.address',
    title: 'Focus the address',
    category: 'navigation',
    run: () => document.querySelector<HTMLInputElement>('.topbar-conn input')?.select(),
  },
  {
    id: 'collections.refresh',
    title: 'Refresh collections',
    category: 'general',
    run: s => void s.refreshCollections(),
  },
];

function stepTab(s: PlayStore, delta: number) {
  if (!s.activeTabId || s.tabs.length < 2) return;
  const i = s.tabs.findIndex(t => t.id === s.activeTabId);
  const next = (i + delta + s.tabs.length) % s.tabs.length;
  s.setActiveTab(s.tabs[next].id);
}

export function matchesCommand(title: string, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const haystack = title.toLowerCase();
  let i = 0;
  for (const ch of q) {
    if (ch === ' ') continue;
    i = haystack.indexOf(ch, i);
    if (i === -1) return false;
    i++;
  }
  return true;
}

export const SAY_TOAST = {
  ok: 'success',
  bad: 'error',
  note: 'refuse',
} as const;

export function commandRefusal(command: Command, state: PlayStore): string | null {
  if (!command.enabled || command.enabled(state)) return null;
  return command.why?.(state) ?? `${command.title} — not available right now`;
}

export function filterCommands(commands: Command[], query: string, state: PlayStore): Command[] {
  return commands.filter(c => (c.enabled ? c.enabled(state) : true) && matchesCommand(c.title, query));
}

export function hotkeyCommands(): (Command & { hotkey: NonNullable<Command['hotkey']> })[] {
  return COMMANDS.filter((c): c is Command & { hotkey: NonNullable<Command['hotkey']> } => !!c.hotkey);
}
