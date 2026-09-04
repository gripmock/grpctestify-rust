import { Fragment, useRef, useState } from 'react';
import type React from 'react';
import { useStore } from '../../lib/store';
import { callAddress, projectCallEnv, resolveProjectAddress } from '../../lib/store';
import { FlaskConical, Clock, Loader2, Square } from 'lucide-react';
import { runProgressLine } from '../../lib/jobs';
import { copyToClipboard } from 'luvo/data/clipboard';
import { JqGolf } from '../ui/JqGolf';
import { count } from 'luvo/data/plural';

function connectionClass(status: string | undefined) {
  if (status === 'ok') return 'dot is-ok';
  if (status === 'error') return 'dot is-fail';
  return 'dot';
}

const EGG_CLICKS = 5;
const EGG_WINDOW_MS = 2500;

export function StatusBar() {
  const [game, setGame] = useState(false);
  const taps = useRef<number[]>([]);
  const tapVersion = () => {
    const now = Date.now();
    taps.current = [...taps.current, now].filter(t => now - t < EGG_WINDOW_MS);
    if (taps.current.length >= EGG_CLICKS) {
      taps.current = [];
      setGame(true);
    }
  };
  const totalOk = useStore(s => s.totalOk);
  const totalError = useStore(s => s.totalError);
  const version = useStore(s => s.version);
  const lastResponse = useStore(s => s.response);
  const projectRoot = useStore(s => s.projectRoot);
  const projectRootAbs = useStore(s => s.projectRootAbs);
  const collectionsDir = useStore(s => s.collectionsDir);
  const envNames = useStore(s => s.projectEnvNames);
  const sessionId = useStore(s => s.sessionId);
  const address = useStore(s => resolveProjectAddress(callAddress(s), projectCallEnv(s)));
  const dialled = useStore(s => s.lastCallAddress);
  const showSidebarTab = useStore(s => s.showSidebarTab);
  const run = useStore(s => s.run);
  const runJobId = useStore(s => s.runJobId);
  const cancelRun = useStore(s => s.cancelRun);
  const running = runJobId !== null && !run.finished;

  const left: Item[] = [
    {
      key: 'version',
      node: (
        <button className="btn is-ghost is-sm status-version" onClick={tapVersion} title="grpctestify">
          <FlaskConical size={12} className="brand-mark" />
          <span className="mono">{version ? `v${version}` : 'grpctestify'}</span>
        </button>
      ),
    },
  ];

  if (totalOk + totalError > 0) {
    left.push({
      key: 'totals',
      node: (
        <button
          className="btn is-ghost is-sm status-totals"
          onClick={() => showSidebarTab('history')}
          title={`${count(totalOk + totalError, 'call')} from this browser — open History`}
        >
          <Clock size={11} />
          {totalOk > 0 && <span className="count is-ok">✓ {totalOk}</span>}
          {totalError > 0 && <span className="count is-fail">✗ {totalError}</span>}
        </button>
      ),
    });
  }

  if (running) {
    left.push({
      key: 'run',
      node: (
        <span className="bar status-run">
            <Loader2 size={11} className="animate-spin" />
            <span>{runProgressLine(run)}</span>
            <button
              className="btn is-ghost is-sm is-icon"
              onClick={() => void cancelRun()}
              title={run.kind === 'bench'
                ? 'Stop the measurement — in-flight requests finish and the report says it was cancelled'
                : 'Stop the run — the call in flight is dropped and the rest of the files are skipped'}
              aria-label="Cancel the run"
            >
              <Square size={11} />
            </button>
        </span>
      ),
    });
  }

  if (projectRoot) {
    const where = projectRootAbs ?? projectRoot;
    left.push({
      key: 'project',
      node: (
        <button
          className="btn is-ghost is-sm status-project"
          title={[
            `Serving the project at ${where}`,
            collectionsDir ? `Files: ${collectionsDir}` : null,
            envNames.length > 0 ? `Environments: ${envNames.join(', ')}` : 'No environments yet',
            'Click to copy the path',
          ].filter(Boolean).join('\n')}
          onClick={() => {
            void copyToClipboard(where).catch(() => {});
          }}
        >
          <span className="badge is-info">.grpctestify</span>
        </button>
      ),
    });
  }

  const right: Item[] = [];

  if (sessionId && projectRoot) {
    right.push({
      key: 'sid',
      node: (
        <button
          className="btn is-ghost is-sm mono"
          onClick={() => void copyToClipboard(sessionId).catch(() => {})}
          title="This browser session — the project's history files are tagged with it. Click to copy."
        >
          sid {sessionId}
        </button>
      ),
    });
  }

  right.push({
    key: 'shortcuts',
    node: (
      <button
        className="btn is-ghost is-sm status-keys"
        onClick={() => useStore.getState().setShowHotkeyHelp(true)}
        title="Every shortcut, and the commands behind them"
        aria-label="Keyboard shortcuts"
      >
        <kbd className="kbd">?</kbd>
      </button>
    ),
  });

  right.push({
    key: 'target',
    node: (
      <span
        className="bar status-target"
        title={
          !dialled ? `Calls go to ${address} — nothing has been sent yet`
          : dialled === address ? `The last call went to ${dialled}`
          : `The last call went to ${dialled} — calls now go to ${address}`
        }
      >
        <span className={dialled ? connectionClass(lastResponse?.status) : 'dot'} />
        <span className="mono">{dialled ?? address}</span>
        {dialled && dialled !== address && <span className="muted">last call</span>}
      </span>
    ),
  });

  return (
    <>
    {game && <JqGolf onClose={() => setGame(false)} />}
    <footer className="statusbar">
      <Strip items={left} />
      <span className="grow" />
      <Strip items={right} />
    </footer>
    </>
  );
}

type Item = { key: string; node: React.ReactNode };

function Strip({ items }: { items: Item[] }) {
  return (
    <>
      {items.map((item, i) => (
        <Fragment key={item.key}>
          {i > 0 && <span className="sep" aria-hidden="true">|</span>}
          {item.node}
        </Fragment>
      ))}
    </>
  );
}
