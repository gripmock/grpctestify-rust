import { useState } from 'react';
import { useStore } from '../../lib/store';
import { X } from 'lucide-react';
import { attributeMeta, attributeTags, tagsInUse } from '../../lib/meta-tags';

export function MetaEditor() {
  const parsed = useStore(s => s.collectionParsed);
  const setMetaField = useStore(s => s.setMetaField);
  const setMetaTags = useStore(s => s.setMetaTags);
  const setMetaLinks = useStore(s => s.setMetaLinks);
  const [draft, setDraft] = useState('');
  const [linkDraft, setLinkDraft] = useState('');

  const tags = parsed?.meta_tags ?? [];
  const links = parsed?.meta_links ?? [];
  const fromAttributes = attributeTags(parsed?.attributes ?? []);
  const attrOwner = attributeMeta(parsed?.attributes ?? [], 'owner');
  const attrSummary = attributeMeta(parsed?.attributes ?? [], 'summary');
  const collections = useStore(s => s.collections);
  const path = useStore(s => s.workspacePath);
  const known = tagsInUse(collections, [...tags, ...(tags.length === 0 ? fromAttributes : [])], path);

  const addLink = () => {
    const link = linkDraft.trim();
    if (!link || links.includes(link)) { setLinkDraft(''); return; }
    setMetaLinks([...links, link]);
    setLinkDraft('');
  };

  const addTag = () => {
    const t = draft.trim();
    if (!t || tags.includes(t)) { setDraft(''); return; }
    setMetaTags([...tags, t]);
    setDraft('');
  };

  return (
    <div className="stack">
      <label className="stack">
        <span className="label">name</span>
        <input className="field field-frame" placeholder="how a report calls this test"
          value={parsed?.meta_name ?? ''} onChange={e => setMetaField('meta_name', e.target.value)} />
      </label>

      <label className="stack">
        <span className="label">
          summary
          {attrSummary && <span className="muted"> · {(parsed?.meta_summary ?? '') === '' ? 'from #[summary]' : '#[summary] not used'}</span>}
        </span>
        <input className="field field-frame"
          placeholder={attrSummary ?? 'one line'}
          value={parsed?.meta_summary ?? ''} onChange={e => setMetaField('meta_summary', e.target.value)} />
      </label>

      <label className="stack">
        <span className="label">
          owner
          {attrOwner && <span className="muted"> · {(parsed?.meta_owner ?? '') === '' ? 'from #[owner]' : '#[owner] not used'}</span>}
        </span>
        <input className="field field-frame"
          placeholder={attrOwner ?? 'team or person'}
          value={parsed?.meta_owner ?? ''} onChange={e => setMetaField('meta_owner', e.target.value)} />
      </label>

      {((attrOwner && (parsed?.meta_owner ?? '') === '') || (attrSummary && (parsed?.meta_summary ?? '') === '')) && (
        <div className="note">
          A report reads <span className="mono">#[owner]</span> and <span className="mono">#[summary]</span>
          {' '}from the attributes above the sections, and only while this one names none. What is
          written here replaces them.
        </div>
      )}

      <div className="stack">
        <span className="label">
          tags
          {tags.length > 0 && fromAttributes.length > 0 && (
            <span className="muted"> · #[tag] not used</span>
          )}
        </span>
        <div className="bar wrap">
          {tags.length === 0 && fromAttributes.map(t => (
            <span key={t} className="chip is-ghost" title="Written as #[tag] in this file — edit it in the source tab">
              {t}
            </span>
          ))}
          {tags.map(t => (
            <span key={t} className="chip is-on">
              {t}
              <button className="btn is-ghost is-icon" aria-label={`Remove ${t}`}
                onClick={() => setMetaTags(tags.filter(x => x !== t))}>
                <X size={9} />
              </button>
            </span>
          ))}
        </div>
        <input
          className="field field-frame"
          placeholder="tag, then Enter — the names --tags selects in CI"
          value={draft}
          onChange={e => setDraft(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); addTag(); } }}
          onBlur={addTag}
        />
        {tags.length === 0 && fromAttributes.length > 0 && (
          <div className="note">
            This file is selected by <span className="mono">#[tag]</span> attributes. A tag written
            here replaces them — the runner reads the attributes only while this section names none.
          </div>
        )}
        {known.length > 0 && (
          <div className="bar wrap">
            <span className="muted">in this project</span>
            {known.slice(0, 8).map(({ tag, files }) => (
              <button
                key={tag}
                className="btn is-ghost is-sm mono"
                title={`${files} other ${files === 1 ? 'file' : 'files'} carry this tag`}
                onClick={() => setMetaTags([...tags, tag])}
              >
                {tag}
              </button>
            ))}
          </div>
        )}
      </div>

      <div className="stack">
        <span className="label">links</span>
        {links.map((link, i) => (
          <div key={i} className="bar">
            <input
              className="field field-frame grow"
              value={link}
              onChange={e => setMetaLinks(links.map((l, j) => (j === i ? e.target.value : l)))}
              placeholder="https://…"
            />
            <button
              className="btn is-ghost is-icon is-sm"
              onClick={() => setMetaLinks(links.filter((_, j) => j !== i))}
              aria-label={`Remove link ${i + 1}`}
              title="Remove this link"
            >
              <X size={11} />
            </button>
          </div>
        ))}
        <input
          className="field field-frame"
          placeholder="a doc or a ticket, then Enter"
          value={linkDraft}
          onChange={e => setLinkDraft(e.target.value)}
          onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); addLink(); } }}
          onBlur={addLink}
        />
      </div>
    </div>
  );
}
