import { useEffect, useMemo, useRef } from 'react';
import { useStore } from '../../lib/store';
import { AlertTriangle, Copy } from 'lucide-react';
import { lineDiff } from 'luvo/data/diff';
import { Diff } from 'luvo/ui/Diff';
import { copyToClipboard } from 'luvo/data/clipboard';
import { useToast } from 'luvo/ui/useToast';

export function ConflictDialog() {
  const toast = useToast();
  const conflict = useStore(s => s.saveConflict);
  const resolve = useStore(s => s.resolveSaveConflict);
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (conflict && !el.open) el.showModal();
    if (!conflict && el.open) el.close();
  }, [conflict]);

  const diff = useMemo(
    () => lineDiff(conflict?.theirs ?? '', conflict?.mine ?? ''),
    [conflict?.theirs, conflict?.mine],
  );
  const stat = useMemo(
    () => ({
      added: diff.filter(l => l.kind === 'add').length,
      removed: diff.filter(l => l.kind === 'del').length,
    }),
    [diff],
  );

  return (
    <dialog
      ref={ref}
      className="modal is-lg"
      aria-label="File changed on disk"
      onCancel={e => { e.preventDefault(); resolve('cancel'); }}
      onClick={e => { if (e.target === ref.current) resolve('cancel'); }}
      onClose={() => resolve('cancel')}
    >
      {conflict && (
        <>
          <div className="modal-head">
            <h2 className="modal-title bar">
              <AlertTriangle size={14} className="warn" />
              File changed on disk
            </h2>
          </div>

          <div className="modal-body stack">
            <div className="muted">
              <span className="mono">{conflict.path}</span> was written by something else since it
              was opened here.
            </div>

            <div className="bar">
              <span className="field-label grow">what this save would change on disk</span>
              {stat.added > 0 && <span className="badge is-ok">+{stat.added}</span>}
              {stat.removed > 0 && <span className="badge is-fail">−{stat.removed}</span>}
            </div>
            <Diff lines={diff} className="conflict-diff" />

            <details className="conflict-raw">
              <summary>the two versions in full</summary>
              <div className="conflict-panes">
                <div className="stack conflict-side">
                  <div className="field-label">on disk</div>
                  <pre className="diff">{conflict.theirs || '(empty)'}</pre>
                </div>
                <div className="stack conflict-side">
                  <div className="field-label">{conflict.raw ? 'your editor' : 'your request'}</div>
                  <pre className="diff">{conflict.mine || '(empty)'}</pre>
                </div>
              </div>
            </details>
          </div>

          <div className="modal-foot">
            <button
              className="btn is-ghost grow"
              onClick={() => void copyToClipboard(conflict.mine)
                .then(() => toast.success('Your version copied'))
                .catch(() => toast.error('The browser refused the clipboard'))}
              title="Put your version on the clipboard before choosing"
            >
              <Copy size={12} /> copy mine
            </button>
            <button className="btn is-quiet" onClick={() => resolve('cancel')} autoFocus>Cancel</button>
            <button
              className="btn"
              onClick={() => {
                const path = conflict.path;
                void resolve('reload').then(() => {
                  toast.warn(`${path} reloaded from disk — the edits open here were dropped`);
                });
              }}
            >
              Take disk version
            </button>
            <button className="btn is-primary is-danger" onClick={() => resolve('overwrite')}>
              Overwrite with mine
            </button>
          </div>
        </>
      )}
    </dialog>
  );
}
