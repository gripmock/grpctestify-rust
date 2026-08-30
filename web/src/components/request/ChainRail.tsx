import { useEffect, useState } from 'react';
import { bindingsOf, isRequestDirty, useStore } from '../../lib/store';
import { groupsOf } from '../../lib/chain-diagram';
import { stepMarks } from '../../lib/jobs';
import { chainAddressAt } from '../../lib/address';
import { flowLabel, flowTitle, ranValue } from '../../lib/extract-contract';
import { durationLabel } from '../../lib/format';
import { readText, writeText } from 'luvo/data/storage';
import { stepShape } from '../../lib/shape';
import { ArrowRight, Play, ChevronRight, ChevronDown, Plus, Trash2 } from 'lucide-react';
import { useToast } from 'luvo/ui/ToastContext';
import { count } from 'luvo/data/plural';

const OPEN_KEY = 'play.chain.open';

export function ChainRail() {
  const [open, setOpen] = useState(() => {
    return readText(OPEN_KEY) === 'on';
  });
  useEffect(() => { writeText(OPEN_KEY, open ? 'on' : 'off'); }, [open]);

  const documents = useStore(s => s.documents);
  const activeStep = useStore(s => s.activeStep);
  const selectStep = useStore(s => s.selectStep);
  const toast = useToast();
  const address = useStore(s => s.address);
  const workspacePath = useStore(s => s.workspacePath);
  const dirty = useStore(isRequestDirty);
  const verdict = useStore(s => (s.workspacePath ? s.run.verdicts[s.workspacePath] : undefined));
  const extracted = useStore(bindingsOf);
  const runJobId = useStore(s => s.runJobId);
  const startRun = useStore(s => s.startRun);
  const editChain = useStore(s => s.editChain);
  const [busy, setBusy] = useState(false);
  const edit = async (op: 'append' | 'delete', index = 0) => {
    setBusy(true);
    try {
      const refused = await editChain(op, index);
      if (refused) { toast.error(refused); return; }
      const grown = useStore.getState().documents;
      const last = grown[grown.length - 1];
      const unverified = op === 'append' && (last?.asserts.length ?? 0) === 0;
      toast.success(op === 'append'
        ? `Step ${grown.length} written to ${workspacePath}${unverified ? ' — nothing verifies it yet' : ''}`
        : `Step ${index + 1} removed from ${workspacePath}`);
    } finally {
      setBusy(false);
    }
  };

  if (documents.length === 0) return null;
  const single = documents.length < 2;

  const marks = stepMarks(verdict, documents.length);
  const grouped = new Set(
    groupsOf(documents.map(d => ({ parallel: d.parallel === true })))
      .flatMap(g => Array.from({ length: g.end - g.start + 1 }, (_, k) => g.start + k)),
  );
  const failedAt = marks.findIndex(m => m.state === 'fail');
  const canRun = !!workspacePath && runJobId === null;

  return (
    <section className={`chain${open ? ' is-open' : ''}`}>
      <div className="chain-bar">
        <button
          className="btn is-sm is-ghost chain-toggle"
          onClick={() => setOpen(v => !v)}
          aria-expanded={open}
          title="fail-fast — a failed step skips the rest"
        >
          {open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
          {single ? 'one step' : `chain · ${documents.length} steps`}
        </button>

        {!single && <span className="rail-dots">
          {marks.map((m, i) => (
            <button
              key={i}
              className={`rail-dot is-${m.state}${i === activeStep ? ' is-on' : ''}${
                grouped.has(i) ? ' is-grouped' : ''}`}
              aria-label={`Step ${i + 1}`}
              aria-pressed={i === activeStep}
              onClick={() => {
                if (!selectStep(i)) toast.refuse('Save or discard this step before moving to another');
              }}
              title={`Step ${i + 1}: ${documents[i]?.endpoint || '(no endpoint)'}${
                grouped.has(i) ? ' — goes out with the steps beside it' : ''}${
                m.state === 'fail' ? ' — failed' : m.state === 'skip' ? ' — skipped' : ''}`}
            />
          ))}
        </span>}

        {failedAt >= 0 && <span className="muted">stopped at step {failedAt + 1}</span>}

        <span className="grow" />

        {open && activeStep > 0 && (
          <button
            className="btn is-sm is-ghost"
            disabled={!canRun}
            title={workspacePath
              ? `Run steps 1–${activeStep + 1} of the saved file`
              : 'Save the file first — a run reads it from disk'}
            onClick={() => workspacePath && startRun([workspacePath], activeStep + 1)}
          >
            <Play size={11} /> run to here
          </button>
        )}
        <button
          className="btn is-sm is-ghost"
          disabled={!canRun}
          onClick={() => workspacePath && startRun([workspacePath])}
          title={workspacePath ? 'Run the whole chain' : 'Save the file first — a run reads it from disk'}
        >
          <Play size={11} /> run
        </button>
        {(open || single) && (
          <button
            className="btn is-sm is-ghost"
            disabled={busy || !workspacePath}
            onClick={() => void edit('append')}
            title={workspacePath
              ? single
                ? 'Add a second step — this file becomes a chain, and the step starts from the same endpoint'
                : 'Add a step after the last one — it starts from the same endpoint, and is shaped like it'
              : 'Save the file first — a chain lives in one'}
          >
            <Plus size={11} /> step
          </button>
        )}
      </div>

      {open && !single && (
        <div className="rail">
          {documents.map((doc, i) => {
            const mark = marks[i];
            const selected = i === activeStep;
            const removable = selected && documents.length > 1;
            return (
              <div key={doc.index} className="step-row">
                <button
                  className={`step${selected ? ' is-on' : ''}${mark.state === 'none' ? '' : ` is-${mark.state === 'pass' ? 'ok' : mark.state}`}`}
                  onClick={() => {
                    if (!selectStep(i)) {
                      toast.refuse('Save or discard this step before moving to another');
                    }
                  }}
                  aria-expanded={selected}
                >
                  <span className="mark">
                    {mark.state === 'pass' ? '✓' : mark.state === 'fail' ? '✗' : mark.state === 'skip' ? '∅' : i + 1}
                  </span>
                  <span className={`badge is-kind ${stepShape(doc).tone}`}>{stepShape(doc).label}</span>
                  <span className="method">{doc.endpoint || '(no endpoint)'}</span>

                  <span className="step-note">
                    {doc.address_source !== 'section' && (
                      <span
                        className="muted"
                        title={`dials ${chainAddressAt(documents, i) || address || '$GRPCTESTIFY_ADDRESS'} — this step has no ADDRESS of its own`}
                      >
                        inherits address
                      </span>
                    )}
                    {doc.consumes.length > 0 && (
                      <span className="mono">uses {doc.consumes.map(v => `{{${v}}}`).join(' ')}</span>
                    )}
                    {doc.bodies.length > 1 && <span>{doc.bodies.length} messages</span>}
                  </span>

                  {doc.produces.length > 0 && i < documents.length - 1 && (
                    <span className="step-flow">
                      <ArrowRight size={9} />
                      {doc.produces.map(v => (
                        <span key={v} className="chip is-on" title={flowTitle(v, ranValue(extracted, v))}>
                          {flowLabel(v, ranValue(extracted, v))}
                        </span>
                      ))}
                    </span>
                  )}

                  <span className="took">
                    {mark.state === 'skip' ? 'skipped'
                      : mark.durationMs !== undefined ? durationLabel(mark.durationMs)
                      : doc.asserts.length > 0 ? count(doc.asserts.length, 'assert') : ''}
                  </span>
                </button>

                {removable && (
                  <button
                    className="btn is-sm is-ghost is-icon step-remove"
                    disabled={busy}
                    onClick={() => void edit('delete', i)}
                    title={`Remove step ${i + 1} from the file — the rest of the chain is kept`}
                    aria-label={`Remove step ${i + 1}`}
                  >
                    <Trash2 size={11} />
                  </button>
                )}
                {selected && dirty && (
                  <div className="note step-detail">
                    Saving writes step {i + 1}; the rest of the chain is preserved as written.
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
