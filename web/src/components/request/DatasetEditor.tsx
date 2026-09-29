import { useState } from 'react';
import type React from 'react';
import { useStore } from '../../lib/store';
import { useToast } from 'luvo/ui/useToast';
import { addColumn, addRow, cellOut, columnsOf, datasetUsage, removeColumn, setCell } from '../../lib/dataset-model';
import { Plus, X } from 'lucide-react';
import { count } from 'luvo/data/plural';

export function DatasetEditor() {
  const parsed = useStore(s => s.collectionParsed);
  const setDataset = useStore(s => s.setDataset);
  const renameDatasetColumn = useStore(s => s.renameDatasetColumn);
  const renameExtractVariable = useStore(s => s.renameExtractVariable);
  const workspacePath = useStore(s => s.workspacePath);
  const toast = useToast();
  const [column, setColumn] = useState('');

  const rows = parsed?.dataset ?? [];
  const columns = columnsOf(rows);
  const request = useStore(s => s.request);
  const usage = datasetUsage(
    columns,
    [...request.bodies, ...Object.values(request.headers), ...(parsed?.expect_responses ?? []).map(r => JSON.stringify(r))],
    [...(parsed?.asserts ?? []), ...Object.values(parsed?.extracts ?? {})],
  );

  const [renaming, setRenaming] = useState<{ from: string; to: string } | null>(null);
  const commitRename = async () => {
    if (!renaming) return;
    const { from, to } = renaming;
    const name = to.trim();
    if (!name || name === from) { setRenaming(null); return; }
    if (workspacePath) {
      const outcome = await renameExtractVariable(from, name, { dataset: true });
      setRenaming(null);
      if ('refused' in outcome) { toast.error(outcome.refused); return; }
      toast.info(`{{dataset.${from}}} renamed to {{dataset.${name}}} — ${count(outcome.rewritten, 'place')}`);
      return;
    }
    const touched = renameDatasetColumn(from, name);
    setRenaming(null);
    if (touched > 0) {
      toast.info(touched === 1
        ? `1 reference renamed to {{dataset.${name}}}`
        : `${touched} references renamed to {{dataset.${name}}}`);
    }
  };

  const cols = {
    gridTemplateColumns: `repeat(${columns.length}, minmax(7rem, 1fr)) auto`,
  } as React.CSSProperties;

  const commitColumn = () => {
    if (!column.trim()) return;
    setDataset(addColumn(rows, column));
    setColumn('');
  };

  return (
    <div className="stack">
      <div className="bar">
        <span className="field-label grow">
          {count(rows.length, 'row')} → {count(rows.length, 'case')}
        </span>
        <button className="btn is-ghost is-sm" onClick={() => setDataset(addRow(rows))}>
          <Plus size={11} /> row
        </button>
      </div>

      {columns.length > 0 && (
        <div className="dataset">
          <div className="dataset-row is-head" style={cols}>
            {columns.map(c => (
              <span key={c} className="dataset-head">
                <input
                  className={`field mono dataset-name${usage.unused.includes(c) ? ' is-unused' : ''}`}
                  value={renaming?.from === c ? renaming.to : c}
                  onChange={e => setRenaming({ from: c, to: e.target.value })}
                  onBlur={() => void commitRename()}
                  onKeyDown={e => {
                    if (e.key === 'Enter') { e.preventDefault(); void commitRename(); }
                    if (e.key === 'Escape') { e.preventDefault(); setRenaming(null); }
                  }}
                  title={usage.unused.includes(c)
                    ? `Nothing reads {{dataset.${c}}} — the rows still multiply the run`
                    : `Read as {{dataset.${c}}}`}
                  spellCheck={false}
                />
                <button
                  className="btn is-ghost is-icon is-sm"
                  onClick={() => setDataset(removeColumn(rows, c))}
                  title={`Remove the ${c} column from every row`}
                  aria-label={`Remove column ${c}`}
                >
                  <X size={10} />
                </button>
              </span>
            ))}
            <span />
          </div>
          {rows.map((row, i) => (
            <div key={i} className="dataset-row" style={cols}>
              {columns.map(c => (
                <input
                  key={c}
                  className="field mono"
                  value={cellOut((row as Record<string, unknown>)[c])}
                  onChange={e => setDataset(setCell(rows, i, c, e.target.value))}
                />
              ))}
              <button className="btn is-ghost is-icon" aria-label={`Remove row ${i + 1}`}
                onClick={() => setDataset(rows.filter((_, j) => j !== i))}>
                <X size={11} />
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="field-frame">
        <input
          className="field mono"
          placeholder="column name, then Enter"
          value={column}
          onChange={e => setColumn(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); commitColumn(); } }}
        />
        <button className="btn is-ghost is-sm" onClick={commitColumn} disabled={!column.trim()}>add column</button>
      </div>

      {usage.missing.length > 0 && (
        <div className="note is-warn">
          {usage.missing.map(name => `{{dataset.${name}}}`).join(', ')} — no such column, so it is sent
          as written. Add the column, or fix the placeholder.
        </div>
      )}

      {usage.inert.length > 0 && (
        <div className="note is-warn">
          {usage.inert.map(name => `{{dataset.${name}}}`).join(', ')} — written in ASSERTS or EXTRACT,
          where nothing is substituted: the expression runs as written and compares against the braces
          themselves. Substitution reaches the request, its headers and the expected response.
        </div>
      )}

      {columns.length === 0 && (
        <div className="note">
          A row per case: every row runs the whole file once, with its fields substituted as
          <span className="mono"> {'{{dataset.<column>}}'}</span> in the request, its headers and the
          expected response. For anything larger than a handful of rows, <span className="mono">run
          --data</span> takes a CSV, TSV or NDJSON file instead.
        </div>
      )}

      {columns.length > 0 && (
        <div className="note">
          Each row becomes one case. Reference a field as
          <span className="mono"> {'{{dataset.' + columns[0] + '}}'}</span> in the request, its headers
          or the expected response — an ASSERTS expression runs as written and substitutes nothing. A
          DATASET in the file and <span className="mono">--data</span> on the command line are
          mutually exclusive.
        </div>
      )}
    </div>
  );
}
