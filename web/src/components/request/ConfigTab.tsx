import { useState } from 'react';
import type React from 'react';
import { useStore } from '../../lib/store';
import { shortPath } from '../../lib/format';
import { GCTF_SECTIONS, configKeys, hiddenSections } from '../../lib/sections';
import { requestFamily } from '../../lib/http-endpoint';
import type { RequestTab } from '../../lib/types';
import { OptionsEditor } from './OptionsEditor';
import { TlsEditor } from './TlsEditor';
import { MetaEditor } from './MetaEditor';
import { ProtoEditor } from './ProtoEditor';
import { DatasetEditor } from './DatasetEditor';
import { BenchEditor } from './BenchEditor';
import { csvJoin } from '../../lib/section-model';
import { ChevronRight, Plus, Trash2 } from 'lucide-react';
import { useModal } from 'luvo/ui/ModalContext';
import { Popover } from 'luvo/ui/Popover';
import { useDismiss } from 'luvo/input/useDismiss';
import { count } from 'luvo/data/plural';

const EDITORS: Partial<Record<RequestTab, () => React.ReactElement>> = {
  options: OptionsEditor,
  tls: TlsEditor,
  meta: MetaEditor,
  proto: ProtoEditor,
  dataset: DatasetEditor,
  bench: BenchEditor,
};

function listOf(keys: string[]): string {
  if (keys.length < 2) return keys.join('');
  return `${keys.slice(0, -1).join(', ')} and ${keys[keys.length - 1]}`;
}

export function ConfigTab() {
  const parsed = useStore(s => s.collectionParsed);
  const bodies = useStore(s => s.request.bodies);
  const headers = useStore(s => s.request.headers);
  const setSectionKv = useStore(s => s.setSectionKv);
  const setDataset = useStore(s => s.setDataset);
  const setMetaField = useStore(s => s.setMetaField);
  const setMetaTags = useStore(s => s.setMetaTags);
  const setMetaLinks = useStore(s => s.setMetaLinks);
  const [open, setOpen] = useState<Set<RequestTab>>(new Set());
  const [adding, setAdding] = useState(false);
  const modal = useModal();

  const toggle = (key: RequestTab) => setOpen(prev => {
    const next = new Set(prev);
    if (!next.delete(key)) next.add(key);
    return next;
  });

  const drop = async (key: RequestTab, label: string) => {
    const ok = await modal.confirm(
      `Remove ${label}?`,
      'The section leaves the file on the next save.',
      { confirmText: 'remove', cancelText: 'keep', danger: true },
    );
    if (!ok) return;
    if (key === 'dataset') setDataset([]);
    else if (key === 'meta') {
      setMetaField('meta_name', '');
      setMetaField('meta_summary', '');
      setMetaField('meta_owner', '');
      setMetaTags([]);
      setMetaLinks([]);
    } else if (key === 'options' || key === 'tls' || key === 'proto' || key === 'bench') {
      setSectionKv(key, {});
    }
    setOpen(prev => { const next = new Set(prev); next.delete(key); return next; });
  };

  const addRef = useDismiss<HTMLDivElement>(adding, () => setAdding(false));
  const family = useStore(s => requestFamily(s.workspacePath, s.request.endpoint));
  const keys = configKeys(family);
  const missing = hiddenSections(parsed, bodies, headers, family).filter(s => keys.includes(s.key));
  const present = keys.filter(k => !missing.some(m => m.key === k) || open.has(k));

  const add = (key: RequestTab) => {
    setAdding(false);
    const seed = sectionSeed(key);
    if (key === 'dataset') setDataset(seed === null ? [] : [{}]);
    else if (seed !== null && (key === 'bench' || key === 'options' || key === 'proto' || key === 'tls')) {
      setSectionKv(key, seed);
    }
    setOpen(prev => new Set(prev).add(key));
  };

  return (
    <div className="stack config">
      {present.map(key => {
        const def = GCTF_SECTIONS.find(s => s.key === key)!;
        const Editor = EDITORS[key]!;
        const isOpen = open.has(key);
        return (
          <div key={key} className={`config-row${isOpen ? ' is-open' : ''}`}>
            <div className="config-headrow">
              <button className="config-head" onClick={() => toggle(key)} aria-expanded={isOpen}>
                <ChevronRight size={11} className={isOpen ? 'tree-caret is-open' : 'tree-caret'} />
                <span className="config-name">{def.label}</span>
                {!isOpen && (
                  <span className="config-summary mono">
                    {summarize(key, parsed) || <span className="muted">not set</span>}
                  </span>
                )}
              </button>
              <button
                className="btn is-ghost is-icon is-sm"
                onClick={() => void drop(key, def.label)}
                title={`Remove ${def.label} from the file`}
                aria-label={`Remove ${def.label}`}
              >
                <Trash2 size={11} />
              </button>
            </div>
            {isOpen && <div className="config-body"><Editor /></div>}
          </div>
        );
      })}

      {present.length === 0 && (
        <div className="muted">
          No config sections — {listOf(keys)} live here when the file has them.
        </div>
      )}

      {missing.length > 0 && (
        <div className="picker" ref={addRef}>
          <button className="btn is-sm is-ghost" onClick={() => setAdding(v => !v)}>
            <Plus size={11} /> add a section
          </button>
          <Popover open={adding} anchor={addRef}>
            <div className="menu">
              {missing.map(s => (
                <button key={s.key} className="menu-item stack is-tall" onClick={() => add(s.key)}>
                  <span>{s.label}</span>
                  {s.note && <span className="muted menu-note">{s.note}</span>}
                </button>
              ))}
            </div>
          </Popover>
        </div>
      )}
    </div>
  );
}

export function sectionSeed(key: RequestTab): Record<string, string> | null {
  if (key === 'bench') return { mode: 'fixed' };
  if (key === 'dataset') return {};
  return null;
}

function summarize(key: RequestTab, p: ReturnType<typeof useStore.getState>['collectionParsed']): string {
  if (!p) return '';
  switch (key) {
    case 'options': return pairs(p.options);
    case 'tls': return pairs(p.tls);
    case 'proto': return pairs(p.proto);
    case 'bench': return pairs(p.bench);
    case 'dataset': {
      const rows = p.dataset ?? [];
      const cols = new Set(rows.flatMap(r => Object.keys(r as object)));
      return rows.length === 0 ? '' : `${count(rows.length, 'row')} · ${[...cols].join(', ')}`;
    }
    case 'meta': {
      const links = p.meta_links?.length ?? 0;
      const bits = [
        p.meta_name,
        p.meta_owner,
        csvJoin(p.meta_tags ?? []),
        links > 0 ? `${count(links, 'link')}` : '',
      ].filter(Boolean);
      return bits.join(' · ');
    }
    default: return '';
  }
}

const PATHY = /^[./~]|\//;

function pairs(o: Record<string, string> | undefined): string {
  const entries = Object.entries(o ?? {});
  if (entries.length === 0) return '';
  const shown = entries
    .slice(0, 3)
    .map(([k, v]) => `${k} ${PATHY.test(v) ? shortPath(v) : v}`)
    .join(' · ');
  return entries.length > 3 ? `${shown} · +${entries.length - 3}` : shown;
}
