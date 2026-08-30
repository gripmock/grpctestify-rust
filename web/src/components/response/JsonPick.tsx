import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { extractAudience } from '../../lib/extract-contract';
import { useStore } from '../../lib/store';
import { useDismiss } from 'luvo/input/useDismiss';
import { useToast } from 'luvo/ui/ToastContext';
import { useModal } from 'luvo/ui/ModalContext';
import { acrossStream, containerActions, isContainer, numberAssert, roundedNote, streamNote } from '../../lib/pick-actions';
import { copyToClipboard } from 'luvo/data/clipboard';

export function JsonPick({ value, messages }: { value: unknown; messages?: unknown[] }) {
  const [menu, setMenu] = useState<{ path: string; value: unknown; x: number; y: number } | null>(null);
  const addAssert = useStore(s => s.addAssert);
  const documents = useStore(s => s.documents);
  const activeStep = useStore(s => s.activeStep);
  const addExtract = useStore(s => s.addExtract);
  const focusAnswerStep = useStore(s => s.focusAnswerStep);
  const setRequestTab = useStore(s => s.setRequestTab);
  const toast = useToast();
  const modal = useModal();

  const triggerRef = useRef<HTMLElement | null>(null);
  const openMenu = (path: string, v: unknown, el: HTMLElement) => {
    triggerRef.current = el;
    const rect = el.getBoundingClientRect();
    setMenu({ path, value: v, x: rect.left, y: rect.bottom + 8 });
  };

  const menuRef = useRef<HTMLDivElement>(null);
  const [placed, setPlaced] = useState<{ left: number; top: number } | null>(null);
  useLayoutEffect(() => {
    if (!menu) { setPlaced(null); return; }
    const el = menuRef.current;
    if (!el) return;
    const { width, height } = el.getBoundingClientRect();
    const margin = 8;
    const left = Math.max(margin, Math.min(menu.x, window.innerWidth - width - margin));
    const below = menu.y + height + margin <= window.innerHeight;
    const top = below ? menu.y : Math.max(margin, menu.y - height - 16);
    setPlaced({ left, top });
  }, [menu]);

  useEffect(() => {
    if (placed) menuRef.current?.querySelector<HTMLButtonElement>('.menu-item')?.focus();
  }, [placed]);

  const close = useCallback(() => {
    setMenu(null);
    triggerRef.current?.focus();
  }, []);
  const pickRef = useDismiss<HTMLDivElement>(menu !== null, close);

  const answerElsewhere = () => {
    const from = useStore.getState().response?.fromStep;
    return from === undefined
      ? 'This step has edits — save or discard them first'
      : `This answer is step ${from + 1}'s — save or discard this step's edits first`;
  };

  const assertLine = (line: string) => {
    if (!focusAnswerStep()) { toast.refuse(answerElsewhere()); close(); return; }
    if (addAssert(line) === 'duplicate') toast.info('This file already asserts that');
    else toast.success('Assertion added — Save writes it to the file');
    setRequestTab('asserts');
    close();
  };

  const assertEq = () => {
    if (!menu) return;
    assertLine(`${menu.path} == ${literal(menu.value)}`);
  };

  const assertShape = () => {
    if (!menu) return;
    assertLine(shapeAssert(menu.path, menu.value));
  };

  const extract = async () => {
    if (!menu) return;
    if (!focusAnswerStep()) { toast.refuse(answerElsewhere()); close(); return; }
    const path = menu.path;
    close();
    const suggested = path.split(/[.[]/).filter(Boolean).pop()?.replace(/\W/g, '') || 'value';
    const name = await modal.prompt(
      'Extract variable',
      `Variable name — ${extractAudience(documents.length, activeStep)}:`,
      suggested,
    );
    if (!name) return;
    addExtract(name, path);
    toast.success(`{{${name}}} = ${path}`);
    setRequestTab('extracts');
  };

  const copyPath = async () => {
    if (!menu) return;
    try {
      await copyToClipboard(menu.path);
      toast.success('Path copied');
    } catch {
      toast.error('The browser refused the clipboard');
    }
    close();
  };

  return (
    <div ref={pickRef} className="pick-tree" onClick={close}>
      <pre className="diff is-flush">
        <Node value={value} path="" depth={0} onPick={openMenu} />
      </pre>

      {menu && (
        <div
          ref={menuRef}
          className="menu pick-menu"
          style={{ left: placed?.left ?? menu.x, top: placed?.top ?? menu.y, visibility: placed ? 'visible' : 'hidden' }}
          onClick={e => e.stopPropagation()}
        >
          <div className="menu-group mono">{menu.path}</div>
          {isContainer(menu.value) ? (
            <>
              {containerActions(menu.path, menu.value).map(action => (
                <button key={action.line} className="menu-item" onClick={() => assertLine(action.line)}>
                  {action.label}
                </button>
              ))}
            </>
          ) : (
            <>
              {(() => {
                const stream = messages ?? [];
                const state = acrossStream(stream, menu.path, menu.value);
                const note = streamNote(state, stream.length) ?? roundedNote(menu.value);
                return (
                  <>
                    <button className="menu-item" onClick={assertEq} disabled={note !== null}>
                      Assert equals {short(literal(menu.value))}
                    </button>
                    {note && <div className="menu-foot pick-note">{note}</div>}
                    {(() => {
                      const cast = numberAssert(menu.path, menu.value);
                      return cast && (
                        <button
                          className="menu-item"
                          onClick={() => assertLine(cast.line)}
                          disabled={note !== null}
                          title="protobuf sends 64-bit integers as strings — this compares the number"
                        >
                          {cast.label}
                        </button>
                      );
                    })()}
                    <button className="menu-item" onClick={assertShape}>{shapeLabel(menu.value)}</button>
                  </>
                );
              })()}
            </>
          )}
          <button className="menu-item" onClick={extract}>Extract as variable…</button>
          <div className="menu-sep" />
          <button className="menu-item" onClick={copyPath}>Copy path</button>
        </div>
      )}
    </div>
  );
}

function Node({ value, path, depth, onPick }: {
  value: unknown; path: string; depth: number;
  onPick: (path: string, v: unknown, el: HTMLElement) => void;
}) {
  const pad = '  '.repeat(depth);

  if (Array.isArray(value)) {
    if (value.length === 0) return <>{'[]'}</>;
    return (
      <>
        {'[\n'}
        {value.map((v, i) => (
          <span key={i}>
            {pad}{'  '}
            <Node value={v} path={`${path}[${i}]`} depth={depth + 1} onPick={onPick} />
            {i < value.length - 1 ? ',' : ''}{'\n'}
          </span>
        ))}
        {pad}{']'}
      </>
    );
  }

  if (value !== null && typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>);
    if (entries.length === 0) return <>{'{}'}</>;
    return (
      <>
        {'{\n'}
        {entries.map(([k, v], i) => (
          <span key={k}>
            {pad}{'  '}
            <button
              type="button"
              className="pick tok-key"
              title={`${childPath(path, k)} — assert or extract this`}
              onClick={e => { e.stopPropagation(); onPick(childPath(path, k), v, e.currentTarget); }}
            >
              "{k}"
            </button>
            {': '}
            <Node value={v} path={childPath(path, k)} depth={depth + 1} onPick={onPick} />
            {i < entries.length - 1 ? ',' : ''}{'\n'}
          </span>
        ))}
        {pad}{'}'}
      </>
    );
  }

  return (
    <button
      type="button"
      className={`pick ${typeof value === 'string' ? 'tok-str' : 'tok-num'}`}
      title={`${path || '.'} — assert or extract this`}
      onClick={e => { e.stopPropagation(); onPick(path || '.', value, e.currentTarget); }}
    >
      {literal(value)}
    </button>
  );
}

export function childPath(parent: string, key: string): string {
  if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) return `${parent}.${key}`;
  const quoted = `["${key.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"]`;
  return parent ? `${parent}${quoted}` : `.${quoted}`;
}

function literal(v: unknown): string {
  if (typeof v === 'string') return JSON.stringify(v);
  if (v === null) return 'null';
  return String(v);
}

function short(s: string, max = 24): string {
  return s.length > max ? s.slice(0, max) + '…' : s;
}

function shapeAssert(path: string, v: unknown): string {
  if (v === null) return `@has_value(${path})`;
  if (typeof v === 'string') {
    if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(v)) return `@is_uuid(${path})`;
    return `${path} != ""`;
  }
  if (typeof v === 'number') return `${path} >= 0`;
  if (typeof v === 'boolean') return `@has_value(${path})`;
  return `@has_value(${path})`;
}

function shapeLabel(v: unknown): string {
  if (v === null) return 'Assert present';
  if (typeof v === 'string') {
    if (/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(v)) return 'Assert is a UUID';
    return 'Assert non-empty';
  }
  if (typeof v === 'number') return 'Assert >= 0';
  return 'Assert present';
}
