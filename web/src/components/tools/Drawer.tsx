import { useEffect, useRef, useState } from 'react';
import { useStore } from '../../lib/store';
import { useKept } from '../../lib/tool-scratch';
import { isHttpRequest } from '../../lib/http-endpoint';
import { JqTester } from './JqTester';
import { RegexTester } from './RegexTester';
import { SchemaView } from './SchemaView';
import { X } from 'lucide-react';
import { seedLabel, seedMessage } from '../../lib/response-seed';
import { Splitter } from 'luvo/ui/Splitter';
import { Tabs } from 'luvo/ui/Tabs';
import { readNumber, writeText } from 'luvo/data/storage';

type Tool = 'jq' | 'regex' | 'schema';

const MIN_H = 260;
const MAX_H = 620;

export function drawerHeight(kept: number, viewport: number): number {
  const room = Math.max(MIN_H, Math.round(viewport * 0.5));
  return Math.min(Math.max(MIN_H, kept), MAX_H, room);
}

export function Drawer() {
  const open = useStore(s => s.drawerOpen);
  const setOpen = useStore(s => s.setDrawerOpen);
  const response = useStore(s => s.response);
  const selected = useStore(s => s.responseMessage);
  const endpoint = useStore(s => s.request.endpoint);
  const [tool, setTool] = useKept<Tool>('drawer.tool', () => 'jq');
  const seed = useStore(s => s.jqSeed);
  useEffect(() => { if (seed) setTool('jq'); }, [seed, setTool]);
  const isHttp = useStore(s => isHttpRequest(s.workspacePath, s.request.endpoint));
  const tools: Tool[] = isHttp ? ['jq', 'regex'] : ['jq', 'regex', 'schema'];
  useEffect(() => { if (isHttp && tool === 'schema') setTool('jq'); }, [isHttp, tool, setTool]);
  const [kept, setKept] = useState(() => readNumber('play.drawer.h', 360, MIN_H, MAX_H));
  const [viewport, setViewport] = useState(() => (typeof window === 'undefined' ? 900 : window.innerHeight));
  useEffect(() => {
    const onResize = () => setViewport(window.innerHeight);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);
  const height = drawerHeight(kept, viewport);
  const dragging = useRef<{ startY: number; startH: number } | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  useEffect(() => { if (open) panelRef.current?.focus(); }, [open]);

  useEffect(() => {
    const move = (e: MouseEvent) => {
      if (!dragging.current) return;
      const next = Math.min(MAX_H, Math.max(MIN_H, dragging.current.startH + (dragging.current.startY - e.clientY)));
      setKept(next);
    };
    const up = () => {
      if (!dragging.current) return;
      dragging.current = null;
      document.body.style.userSelect = '';
    };
    document.addEventListener('mousemove', move);
    document.addEventListener('mouseup', up);
    return () => { document.removeEventListener('mousemove', move); document.removeEventListener('mouseup', up); };
  }, []);

  useEffect(() => { writeText('play.drawer.h', String(kept)); }, [kept]);

  if (!open) return null;

  const input = seedMessage(response, selected);
  const which = seedLabel(response, selected);

  return (
    <div
      className="drawer"
      ref={panelRef}
      tabIndex={-1}
      onKeyDown={e => { if (e.key === 'Escape') { e.stopPropagation(); setOpen(false); } }}
      style={{ height }}
    >
      <Splitter
        className="hsplit"
        orientation="horizontal"
        label="Tools height"
        title="Drag to resize · arrows to nudge"
        value={height}
        min={MIN_H}
        max={MAX_H}
        step={24}
        invert
        onValue={setKept}
        onMouseDown={e => {
          dragging.current = { startY: e.clientY, startH: height };
          document.body.style.userSelect = 'none';
        }}
      />
      <div className="drawer-head">
        <Tabs
          label="Which tool"
          items={tools.map(t => ({ key: t, label: t }))}
          value={tool}
          onChange={setTool}
        />
        <span className="grow" />
        <span className="muted">
          {tool === 'jq'
            ? (input ? `seeded from the last response${which ? ` · ${which}` : ''}` : 'paste any JSON — a response is not required')
            : tool === 'regex' ? 'matches any text — a message field, a header, an error'
            : endpoint || 'no method chosen'}
        </span>
        <button className="btn is-ghost is-icon" onClick={() => setOpen(false)} aria-label="Close tools">
          <X size={12} />
        </button>
      </div>
      <div className="drawer-body">
        {tool === 'jq' ? <JqTester seed={input} messages={response?.messages ?? []} handed={seed} />
          : tool === 'regex' ? <RegexTester seed={input} />
          : <SchemaView />}
      </div>
    </div>
  );
}
