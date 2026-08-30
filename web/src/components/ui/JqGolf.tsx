import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { X, ChevronRight } from 'lucide-react';
import { readNumber, writeText } from 'luvo/data/storage';
import { useDebouncedPost } from 'luvo/data/useDebouncedPost';
import {
  clockLabel, emptyRound, makeHole, missed, reads, scoreLine, scored, secondsFor, solved, startClock, verdictOf,
  type Hole, type Round,
} from '../../lib/golf';
import { count } from 'luvo/data/plural';

const BEST_KEY = 'play.golf.best';
const rng = () => Math.random();

interface QueryOut {
  outputs: unknown[];
  error: string | null;
  elapsed_us: number;
}

export function JqGolf({ onClose }: { onClose: () => void }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [hole, setHole] = useState<Hole>(() => makeHole(rng));
  const [expr, setExpr] = useState('');
  const [left, setLeft] = useState(() => secondsFor(hole));
  const [carried, setCarried] = useState(0);
  const [round, setRound] = useState<Round>(() =>
    emptyRound(readNumber(BEST_KEY, 0, 0, Number.MAX_SAFE_INTEGER)));

  useEffect(() => { ref.current?.showModal(); }, []);
  useEffect(() => { writeText(BEST_KEY, String(round.best)); }, [round.best]);

  const { data, busy } = useDebouncedPost<QueryOut>(
    '/api/eval/query',
    expr.trim() && left > 0 ? { input: hole.input, expr: expr.trim(), runs: 1 } : null,
    200,
  );

  const outputs = useMemo(() => data?.outputs ?? [], [data]);
  const error = data?.error ?? null;
  const looksSolved = left > 0 && solved(hole, outputs);

  const { data: onDecoy } = useDebouncedPost<QueryOut>(
    '/api/eval/query',
    looksSolved ? { input: hole.decoy, expr: expr.trim(), runs: 1 } : null,
    0,
  );
  const decoyOutputs = useMemo(() => (onDecoy ? onDecoy.outputs : null), [onDecoy]);
  const honest = decoyOutputs !== null && reads(hole, decoyOutputs);
  const done = looksSolved && honest;

  const verdict = verdictOf(hole, expr, outputs, error, decoyOutputs);
  const strokes = expr.trim().length;
  const out = left === 0 && !done;

  const next = useCallback(() => {
    const fresh = makeHole(rng);
    setHole(fresh);
    setLeft(startClock(fresh, carried));
    setExpr('');
  }, [carried]);

  useEffect(() => {
    if (left <= 0 || done) return;
    const handle = setTimeout(() => setLeft(current => current - 1), 1000);
    return () => clearTimeout(handle);
  }, [left, done]);

  const counted = useRef(false);
  useEffect(() => { counted.current = false; }, [hole]);

  useEffect(() => {
    if (counted.current) return;
    if (done) {
      counted.current = true;
      setCarried(left);
      setRound(current => scored(current, hole, strokes, left));
    } else if (left === 0) {
      counted.current = true;
      setCarried(0);
      setRound(missed);
    }
  }, [done, left, hole, strokes]);

  return (
    <dialog
      ref={ref}
      className="modal golf"
      aria-label="jq golf"
      onCancel={e => { e.preventDefault(); onClose(); }}
      onClose={() => onClose()}
      onClick={e => { if (e.target === ref.current) onClose(); }}
    >
      <div className="modal-head">
        <h2 className="modal-title">jq golf</h2>
        <span className="muted golf-round">
          {round.solved} solved{round.missed > 0 ? ` · ${round.missed} missed` : ''}
          {round.solved > 0 ? ` · ${count(round.strokes, 'stroke')} (par ${round.par})` : ''} · best {round.best}
        </span>
        <button className="btn is-ghost is-icon" onClick={onClose} aria-label="Close"><X size={14} /></button>
      </div>

      <div className="modal-body stack">
        <div className="golf-ask">
          <span className="label">{hole.kind}</span>
          <span>{hole.ask}</span>
          <span className="grow" />
          <span className="muted mono">par {hole.par}</span>
          {carried > 0 && <span className="ok mono golf-carry">+{carried}s</span>}
          <span className={`golf-clock mono${out ? ' is-out' : left <= 10 ? ' is-low' : ''}`}>
            {done ? '✓' : clockLabel(left)}
          </span>
        </div>

        <div className="golf-bar" aria-hidden="true">
          <span className={`golf-fill${out ? ' is-out' : left <= 10 ? ' is-low' : ''}`}
            style={{ ['--left' as string]: `${Math.max(0, left) / startClock(hole, carried)}` }} />
        </div>

        <pre className="golf-input mono">{JSON.stringify(hole.input, null, 2)}</pre>

        <div className={`field-frame golf-field${done ? ' is-ok' : out || verdict === 'error' ? ' is-bad' : ''}`}>
          <span className="label">jq</span>
          <input
            className="field mono"
            value={expr}
            autoFocus
            autoComplete="off"
            spellCheck={false}
            disabled={out}
            placeholder="your filter"
            onChange={e => setExpr(e.target.value)}
            onKeyDown={e => { if (e.key === 'Enter' && (done || out)) next(); }}
          />
          <span className="muted mono golf-strokes">{strokes}</span>
        </div>

        <div className="golf-out mono">
          {out ? <span className="fail">out of time — Enter takes the next one</span>
            : verdict === 'empty' ? <span className="muted">type an expression — the real engine runs it</span>
            : busy ? <span className="muted">…</span>
            : verdict === 'error' ? <span className="fail">{error}</span>
            : verdict === 'constant' ? <span className="warn">that answer does not read the input</span>
            : done ? <span className="ok">{scoreLine(hole, strokes)} — Enter takes the next one</span>
            : <span className="muted">{outputs.length === 1 ? JSON.stringify(outputs[0]) : count(outputs.length, 'output')}</span>}
        </div>

        <div className="bar">
          <span className="muted golf-want mono">wanted {JSON.stringify(hole.want)}</span>
          <span className="grow" />
          <button className="btn is-ghost is-sm" onClick={next}>
            next hole <ChevronRight size={11} />
          </button>
        </div>
      </div>
    </dialog>
  );
}
