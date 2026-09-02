import { useCallback, useMemo, useState } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { useDismiss } from 'luvo/input/useDismiss';
import { Popover } from 'luvo/ui/Popover';
import { useDebouncedPost } from 'luvo/data/useDebouncedPost';
import { loadRecent, pushRecent, saveRecent } from '../../lib/history-list';
import { useStore } from '../../lib/store';
import { stepPhrase } from '../../lib/assert-line';
import { useKept } from '../../lib/tool-scratch';
import { History, Plus, CornerDownLeft } from 'lucide-react';
import { firstStringPath, valueAtPath } from '../../lib/json-paths';
import { useToast } from 'luvo/ui/useToast';
import { escapeFor, extractLines, withInlineFlags } from '../../lib/regex-lines';
import { count } from 'luvo/data/plural';

type RegexOut = {
  rewritten_pattern: string;
  matched: boolean;
  spans: [number, number][];
  captures: [string, string][];
  error: string | null;
};

type Mode = 'matches' | 'jq';

const RECENT_KEY = 'play.regex.recent';

const FLAG_HELP: Record<string, string> = {
  i: 'case-insensitive',
  m: '^ and $ match line boundaries',
  s: '. matches newline',
  x: 'ignore whitespace in the pattern',
  u: 'unicode classes',
  U: 'swap greedy and lazy',
};

const REFERENCE: [string, string][] = [
  ['\\d', 'digit'],
  ['\\w', 'word character'],
  ['\\s', 'whitespace'],
  ['.', 'any character'],
  ['a?', 'optional'],
  ['a+', 'one or more'],
  ['a*', 'zero or more'],
  ['a{2,4}', 'between 2 and 4'],
  ['[a-f0-9]', 'character class'],
  ['(?<name>…)', 'named group — jq mode only'],
  ['^ $', 'start / end'],
  ['(?i)', 'inline flag'],
];

export function RegexTester({ seed }: { seed: unknown | null }) {
  const steps = useStore(s => s.documents.length);
  const activeStep = useStore(s => s.activeStep);
  const where = stepPhrase(steps, activeStep);

  const addAssert = useStore(s => s.addAssert);
  const addExtract = useStore(s => s.addExtract);
  const toast = useToast();
  const [pattern, setPattern] = useKept('regex.pattern', () => '');
  const [flags, setFlags] = useKept('regex.flags', () => '');
  const [subject, setSubject] = useKept('regex.subject', () => (typeof seed === 'string' ? seed : ''));
  const [mode, setMode] = useKept<Mode>('regex.mode', () => 'matches');
  const [field, setField] = useKept('regex.field', () => firstStringPath(seed) ?? '.message');
  const [recent, setRecent] = useState<string[]>(() => loadRecent(RECENT_KEY));
  const [showRecent, setShowRecent] = useState(false);
  const recentRef = useDismiss<HTMLDivElement>(showRecent, useCallback(() => setShowRecent(false), []));

  const { data, error, busy } = useDebouncedPost<RegexOut>(
    '/api/eval/regex',
    pattern && subject ? { pattern, flags, subject, mode } : null,
  );

  const commit = () => {
    if (!pattern.trim()) return;
    const next = pushRecent(recent, pattern);
    setRecent(next);
    saveRecent(RECENT_KEY, next);
  };

  const fromResponse = useMemo(() => {
    if (seed === null) return null;
    const at = valueAtPath(seed, field);
    if (at === undefined)
      return {
        text: JSON.stringify(seed, null, 2),
        why: `${field} is not in the last response — takes the whole message`,
      };
    return {
      text: typeof at === 'string' ? at : JSON.stringify(at),
      why: `Takes ${field} from the last response`,
    };
  }, [seed, field]);

  const known = [...flags].filter(f => f in FLAG_HELP);
  const dropped = [...flags].filter(f => !(f in FLAG_HELP));
  const failure = error ?? data?.error ?? null;

  const assertion = useMemo(() => {
    const escaped = escapeFor(pattern);
    if (mode === 'jq') {
      const group = data?.captures[0]?.[0];
      return group && !/^\d+$/.test(group)
        ? `(${field} | capture("${escaped}").${group}) != ""`
        : `${field} | test("${escaped}"${flags ? `; "${flags}"` : ''})`;
    }
    return `${field} matches "${escapeFor(withInlineFlags(pattern, flags))}"`;
  }, [pattern, flags, mode, field, data]);

  const extraction = useMemo(
    () => (mode === 'jq' ? extractLines(pattern, field, data?.captures ?? []) : []),
    [mode, pattern, field, data],
  );

  return (
    <div className="stack">
      <div className="stack is-tight">
        <div className="bar">
          <span className="field-label grow">pattern</span>
          <Seg
            label="Which engine runs the pattern"
            value={mode}
            onChange={setMode}
            options={[
              { value: 'matches', label: 'matches', title: 'The regex crate — what `matches` and @regex run in a .gctf. Yes/no and where, no captures.' },
              { value: 'jq', label: 'jq', title: 'jq test/capture — named groups, usable in EXTRACT' },
            ]}
          />
          <div className="picker" ref={recentRef}>
            <button
              className="btn is-sm is-ghost"
              disabled={recent.length === 0}
              title={recent.length === 0 ? 'Patterns you run are remembered here' : `${count(recent.length, 'pattern')} this browser has run`}
              onClick={() => setShowRecent(v => !v)}
            >
              <History size={11} /> history
            </button>
            <Popover open={showRecent} anchor={recentRef} align="end" className="tool-menu">
              <div className="menu">
                {recent.map(r => (
                  <button key={r} className="menu-item mono" onClick={() => { setPattern(r); setShowRecent(false); }}>{r}</button>
                ))}
                <div className="menu-sep" />
                <button
                  className="menu-item"
                  onClick={() => { setRecent([]); saveRecent(RECENT_KEY, []); setShowRecent(false); }}
                >
                  Forget these
                </button>
              </div>
            </Popover>
          </div>
        </div>

        <div className="bar">
          <div className={`field-frame grow expr-frame${failure ? ' is-bad' : ''}`}>
            <span className="expr-gutter mono">/</span>
            <input
              className="field mono"
              spellCheck={false}
              placeholder="pattern"
              value={pattern}
              onChange={e => setPattern(e.target.value)}
              onBlur={commit}
            />
            <span className="expr-gutter mono">/</span>
          </div>
          <div className="field-frame is-tiny">
            <input className="field mono" placeholder="flags" value={flags} onChange={e => setFlags(e.target.value)} />
          </div>
          {busy && <span className="muted">…</span>}
        </div>

        {(known.length > 0 || dropped.length > 0 || (data && data.rewritten_pattern !== pattern)) && (
          <div className="bar wrap tool-flags">
            {known.map(f => <span key={f} className="chip mono">{f} — {FLAG_HELP[f]}</span>)}
            {dropped.map(f => (
              <span key={f} className="chip mono is-warn" title="Not supported by this engine — silently dropped">
                {f} — dropped
              </span>
            ))}
            {data && data.rewritten_pattern !== pattern && (
              <span className="muted">runs as <span className="mono">{data.rewritten_pattern}</span></span>
            )}
          </div>
        )}

        {failure && <div className="assert is-fail"><span className="assert-mark">!</span><span>{failure}</span></div>}
      </div>

      <div className="tool-grid">
        <div className="stack is-cell">
          <div className="bar">
            <span className="field-label grow">subject</span>
            <button
              className="btn is-sm is-ghost"
              disabled={fromResponse === null}
              title={fromResponse === null ? 'Nothing has been executed yet' : fromResponse.why}
              onClick={() => fromResponse && setSubject(fromResponse.text)}
            >
              <CornerDownLeft size={11} /> from response
            </button>
          </div>
          <textarea
            className="field field-frame code-input tool-subject"
            spellCheck={false}
            placeholder="text to match against"
            value={subject}
            onChange={e => setSubject(e.target.value)}
          />
          {data && !failure && subject && (
            <div className="mono regex-subject">{highlight(subject, data.spans)}</div>
          )}
        </div>

        <div className="stack is-cell">
          <div className="bar">
            <span className="field-label grow">matches</span>
            {data && !failure && (
              <span className={`badge ${data.matched ? 'is-ok' : 'is-fail'}`}>
                {data.matched ? `${data.spans.length} match${data.spans.length !== 1 ? 'es' : ''}` : 'no match'}
              </span>
            )}
          </div>

          {!pattern.trim() && (
            <div className="note">Type a pattern — it runs as you type, against the subject on the left.</div>
          )}
          {pattern.trim() && !subject && (
            <div className="note">Nothing to match against — paste text, or take it from the last response.</div>
          )}

          {data && !failure && data.spans.length > 0 && (
            <div className="matches">
              {data.spans.map(([start, end], i) => (
                <div key={i} className="match-row">
                  <span className="muted mono">#{i + 1}</span>
                  <span className="mono grow">{subject.slice(start, end)}</span>
                  <span className="muted mono">{start}–{end}</span>
                </div>
              ))}
            </div>
          )}

          {data && !failure && data.captures.length > 0 && (
            <div>
              <div className="field-label">groups</div>
              <dl className="kv">
                {data.captures.map(([name, text]) => (
                  <div key={name} className="bar"><dt>{/^\d+$/.test(name) ? `group ${name}` : name}</dt><dd className="mono">{text}</dd></div>
                ))}
              </dl>
            </div>
          )}

          {mode === 'matches' && /\((?!\?[:=!])/.test(pattern) && (
            <div className="note">
              This engine answers yes/no and where — it has no capture groups. Switch to
              <span className="mono"> jq </span> for <span className="mono">capture</span>, which is
              what EXTRACT can bind.
            </div>
          )}

          <div>
            <div className="field-label">quick reference</div>
            <div className="bar wrap tool-flags">
              {REFERENCE.map(([token, meaning]) => (
                <button key={token} className="chip mono" onClick={() => setPattern(p => p + token)} title={meaning}>
                  {token}
                </button>
              ))}
            </div>
          </div>
        </div>
      </div>

      <div className="assert-preview">
        <div className="bar">
          <div className="field-frame is-mid">
            <input className="field mono" value={field} onChange={e => setField(e.target.value)} title="Field to match against" />
          </div>
          <span className="grow" />
          <button
            className="btn is-sm is-ghost"
            disabled={extraction.length === 0 || failure !== null}
            title={
              !pattern.trim() ? 'The pattern is empty'
              : failure ? 'The pattern has to compile first'
              : mode === 'matches' ? 'This engine has no capture groups — switch to jq'
              : extraction.length === 0 ? 'Name a group — (?<id>…) — and EXTRACT can bind it'
              : `Writes ${extraction.map(([name]) => name).join(', ')} into ${where ? `${where}'s` : "the open file's"} EXTRACT`
            }
            onClick={() => {
              for (const [name, expr] of extraction) addExtract(name, expr);
              commit();
              toast.success(`${extraction.map(([name]) => `{{${name}}}`).join(' ')} bound — rename them in EXTRACT`);
            }}
          >
            <Plus size={11} /> add to EXTRACT
          </button>
          <button
            className="btn is-sm"
            disabled={!pattern.trim() || failure !== null}
            title={
              !pattern.trim() ? 'The pattern is empty'
              : failure ? 'The pattern has to compile first'
              : `Writes this line into ${where ? `${where}'s` : "the open file's"} ASSERTS`
            }
            onClick={() => {
              const said = addAssert(assertion);
              commit();
              if (said === 'duplicate') toast.info('This file already asserts that');
              else toast.success('Assertion added — Save writes it to the file');
            }}
          >
            <Plus size={11} /> add to ASSERTS
          </button>
        </div>
        <pre className={`diff assert-line${pattern.trim() ? '' : ' muted'}`}>
          {pattern.trim() ? assertion : 'A pattern first — the line is built from it.'}
        </pre>
        {extraction.length > 0 && (
          <pre className="diff assert-line">
            {extraction.map(([name, expr]) => `${name} = ${expr}`).join('\n')}
          </pre>
        )}
      </div>
    </div>
  );
}

function highlight(subject: string, spans: [number, number][]) {
  const out: React.ReactNode[] = [];
  let pos = 0;
  for (const [start, end] of spans) {
    if (start > pos) out.push(subject.slice(pos, start));
    out.push(<mark key={`${start}-${end}`} className="regex-hit">{subject.slice(start, end)}</mark>);
    pos = end;
  }
  if (pos < subject.length) out.push(subject.slice(pos));
  return out;
}
