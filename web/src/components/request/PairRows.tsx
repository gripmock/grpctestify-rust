import { Plus, X } from 'lucide-react';
import { emptyForm, type QueryParam } from '../../lib/query';

export function PairRows({ noun, rows, empty, onChange }: {
  noun: string;
  rows: QueryParam[];
  empty: string;
  onChange: (rows: QueryParam[]) => void;
}) {
  const Noun = noun[0].toUpperCase() + noun.slice(1);
  const write = (i: number, part: Partial<QueryParam>) =>
    onChange(rows.map((r, at) => (at === i ? { ...r, ...part } : r)));

  return (
    <div className="menu stack params-menu">
      {rows.length === 0 && <div className="muted">{empty}</div>}
      {rows.map((row, i) => (
        <div className="bar" key={i}>
          <input
            className="field mono"
            value={row.key}
            placeholder="name"
            aria-label={`${Noun} ${i + 1} name`}
            spellCheck={false}
            onChange={e => write(i, { key: e.target.value })}
          />
          <input
            className="field mono"
            value={row.value}
            placeholder="value"
            aria-label={`${Noun} ${i + 1} value`}
            spellCheck={false}
            onChange={e => write(i, { value: e.target.value })}
          />
          {row.value === '' && row.key.trim() !== '' && (
            <button
              className="btn is-ghost is-sm mono param-empty"
              title={row.bare === false
                ? 'Sent as `name=`, an empty value — click to send the name alone'
                : 'Sent as the name alone, with no `=` — click to send an empty value'}
              onClick={() => write(i, { bare: row.bare === false })}
            >
              {emptyForm(row)}
            </button>
          )}
          <button
            className="btn is-ghost is-icon is-sm"
            aria-label={`Remove ${noun} ${i + 1}`}
            onClick={() => onChange(rows.filter((_, at) => at !== i))}
          >
            <X size={11} />
          </button>
        </div>
      ))}
      <div className="bar">
        <button className="btn is-sm is-ghost" onClick={() => onChange([...rows, { key: '', value: '' }])}>
          <Plus size={12} /> {noun}
        </button>
      </div>
    </div>
  );
}
