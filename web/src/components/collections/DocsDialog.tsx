import { useEffect, useRef, useState } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { fetchDocs, matchingPages, pageForHref, pageTitle, type DocPage } from '../../lib/docs';
import { useStore } from '../../lib/store';
import { copyToClipboard } from 'luvo/data/clipboard';
import { useToast } from 'luvo/ui/useToast';
import { Copy, Loader2, X } from 'lucide-react';
import { parseMarkdown, type Block, type Inline } from '../../lib/markdown';
import { parseSequence, type Sequence } from '../../lib/mermaid-sequence';
import { tokenizeJson } from 'luvo/data/json-highlight';
import { count } from 'luvo/data/plural';

export function DocsDialog({ paths, onClose }: { paths: string[]; onClose: () => void }) {
  const jobId = useStore(s => s.lastReports.jobId || s.runJobId);
  const ref = useRef<HTMLDialogElement>(null);
  const toast = useToast();
  const [pages, setPages] = useState<DocPage[] | null>(null);
  const [failed, setFailed] = useState<{ paths: string[]; jobId: string | null | undefined; message: string } | null>(null);
  const error = failed && failed.paths === paths && failed.jobId === jobId ? failed.message : null;
  const [at, setAt] = useState(0);
  const [view, setView] = useState<'preview' | 'markdown'>('preview');
  const [filter, setFilter] = useState('');

  useEffect(() => { ref.current?.showModal(); }, []);

  useEffect(() => {
    let live = true;
    fetchDocs(paths, jobId)
      .then(p => { if (live) { setPages(p); setAt(0); } })
      .catch(e => { if (live) setFailed({ paths, jobId, message: e?.message || String(e) }); });
    return () => { live = false; };
  }, [paths, jobId]);

  const page = pages?.[at];
  const shown = pages ? matchingPages(pages, filter) : [];
  const goTo = (p: DocPage) => setAt(pages?.indexOf(p) ?? 0);

  return (
    <dialog
      ref={ref}
      className="modal is-xl"
      aria-label="API documentation"
      onCancel={e => { e.preventDefault(); onClose(); }}
      onClose={() => onClose()}
      onClick={e => { if (e.target === ref.current) onClose(); }}
    >
      <div className="modal-head">
        <h2 className="modal-title">API documentation</h2>
        <span className="muted docs-scope">
          {paths.length === 0 ? 'everything this workbench serves' : `${count(paths.length, 'file')}`}
        </span>
        <button className="btn is-ghost is-icon" onClick={onClose} aria-label="Close"><X size={14} /></button>
      </div>

      <div className="modal-body docs-body">
        {!pages && !error && <div className="empty-state"><Loader2 size={12} className="animate-spin" /> Reading the files…</div>}
        {error && <div className="assert is-fail"><span className="assert-mark">!</span><span>{error}</span></div>}
        {pages && pages.length === 0 && (
          <div className="empty-state">No ENDPOINT-bearing tests here — there is nothing to document.</div>
        )}
        {pages && pages.length > 0 && (
          <>
            <nav className="docs-pages">
              <input
                className="field field-frame docs-filter"
                placeholder={`filter ${count(pages.length - 1, 'page')}`}
                value={filter}
                onChange={e => setFilter(e.target.value)}
                spellCheck={false}
                aria-label="Filter the pages"
              />
              {shown.map(p => (
                <button
                  key={p.name}
                  className={`row${p === page ? ' is-on' : ''}`}
                  onClick={() => goTo(p)}
                  title={p.name}
                >
                  <span className="mono row-name">{pageTitle(p.name, p.markdown)}</span>
                </button>
              ))}
              {shown.length <= 1 && filter.trim() !== '' && (
                <span className="muted docs-none">Nothing names “{filter.trim()}”.</span>
              )}
            </nav>
            <div className="stack docs-page">
              <div className="bar">
                <span className="field-label grow mono" title={page?.name}>
                  {page ? pageTitle(page.name, page.markdown) : ''}
                </span>
                <Seg
                  label="How to read the page"
                  value={view}
                  onChange={setView}
                  options={(['preview', 'markdown'] as const).map(v => ({ value: v, label: v }))}
                />
                <button
                  className="btn is-sm is-ghost"
                  onClick={async () => {
                    if (!page) return;
                    try {
                      await copyToClipboard(page.markdown);
                      toast.success(`${page.name} copied`);
                    } catch {
                      toast.error('The browser refused the clipboard');
                    }
                  }}
                >
                  <Copy size={11} /> copy
                </button>
              </div>
              {view === 'markdown'
                ? <pre className="docs-markdown mono">{page?.markdown}</pre>
                : <div className="docs-preview">{page && <Rendered blocks={parseMarkdown(page.markdown)} pages={pages ?? undefined} onGo={setAt} />}</div>}
            </div>
          </>
        )}
      </div>
    </dialog>
  );
}

function Rendered({ blocks, pages, onGo }: { blocks: Block[]; pages?: DocPage[]; onGo?: (at: number) => void }) {
  const toast = useToast();
  const copyBlock = async (text: string, lang: string) => {
    try {
      await copyToClipboard(text);
      toast.success(lang === 'sh' ? 'Command copied' : `${lang || 'Block'} copied`);
    } catch {
      toast.error('The browser refused the clipboard');
    }
  };
  return (
    <>
      {blocks.map((b, i) => {
        if (b.kind === 'rule') return <hr key={i} className="docs-rule" />;
        if (b.kind === 'list') {
          return (
            <ul key={i} className="docs-list">
              {b.items.map((item, j) => (
                <li key={j}><Line parts={item} pages={pages} onGo={onGo} /></li>
              ))}
            </ul>
          );
        }
        if (b.kind === 'code') {
          const diagram = b.lang === 'mermaid' ? parseSequence(b.text) : null;
          if (diagram) return <Ladder key={i} sequence={diagram} />;
          return (
            <pre key={i} className="docs-code mono">
              {b.lang && <span className="docs-lang">{b.lang}</span>}
              <button
                className="btn is-ghost is-icon docs-copy"
                title={b.lang === 'sh' ? 'Copy this command' : 'Copy this block'}
                aria-label={b.lang === 'sh' ? 'Copy this command' : 'Copy this block'}
                onClick={() => void copyBlock(b.text, b.lang)}
              >
                <Copy size={11} />
              </button>
              {b.lang === 'json'
                ? tokenizeJson(b.text).map((t, j) => (
                    <span key={j} className={t.kind === 'plain' ? undefined : `tok-${t.kind}`}>{t.text}</span>
                  ))
                : b.text}
            </pre>
          );
        }
        if (b.kind === 'heading') {
          const Tag = (b.level === 1 ? 'h1' : b.level === 2 ? 'h2' : 'h3') as 'h1' | 'h2' | 'h3';
          return <Tag key={i} className={`docs-h docs-h${b.level}`}><Line parts={b.text} pages={pages} onGo={onGo} /></Tag>;
        }
        if (b.kind === 'table') {
          return (
            <table key={i} className="docs-table">
              <thead>
                <tr>{b.head.map((c, j) => <th key={j}><Line parts={c} pages={pages} onGo={onGo} /></th>)}</tr>
              </thead>
              <tbody>
                {b.rows.map((row, j) => (
                  <tr key={j}>{row.map((c, k) => <td key={k}><Line parts={c} pages={pages} onGo={onGo} /></td>)}</tr>
                ))}
              </tbody>
            </table>
          );
        }
        return <p key={i} className="docs-p"><Line parts={b.text} pages={pages} onGo={onGo} /></p>;
      })}
    </>
  );
}

function Line({ parts, pages, onGo }: { parts: Inline[]; pages?: DocPage[]; onGo?: (at: number) => void }) {
  return (
    <>
      {parts.map((p, i) => {
        if (p.kind === 'code') return <code key={i} className="mono docs-inline-code">{p.text}</code>;
        if (p.kind === 'strong') return <strong key={i}>{p.text}</strong>;
        if (p.kind === 'link') {
          const at = pages ? pageForHref(pages, p.href) : -1;
          if (at >= 0 && onGo) {
            return (
              <button
                key={i}
                className="docs-link is-page"
                onClick={() => onGo(at)}
                title={`Open ${pageTitle(pages![at].name, pages![at].markdown)}`}
              >
                {p.text}
              </button>
            );
          }
          return <span key={i} className="docs-link" title={p.href}>{p.text}</span>;
        }
        return <span key={i}>{p.text}</span>;
      })}
    </>
  );
}

function Ladder({ sequence }: { sequence: Sequence }) {
  return (
    <div className="docs-ladder">
      <div className="ladder-heads mono">
        {sequence.participants.map(p => <span key={p} className="ladder-head">{p}</span>)}
      </div>
      {sequence.steps.map((step, i) => {
        const from = sequence.participants.indexOf(step.from);
        const to = sequence.participants.indexOf(step.to);
        const back = to < from;
        return (
          <div key={i} className={`ladder-step${step.dashed ? ' is-answer' : ''}`}>
            <span className="ladder-arrow mono">{back ? '←' : '→'}</span>
            <span className="mono ladder-label">{step.label}</span>
            <span className="muted mono ladder-who">{step.from} → {step.to}</span>
          </div>
        );
      })}
    </div>
  );
}
