import { rawAuthorityReason, useStore } from '../../lib/store';
import type { CollectionParsed, RequestTab } from '../../lib/types';
import { sectionsByGroup } from '../../lib/sections';
import { requestFamily } from '../../lib/http-endpoint';
import { Tabs, type TabItem } from 'luvo/ui/Tabs';
import { tabPanelProps } from 'luvo/ui/tab-ids';
import { BodyEditor } from './BodyEditor';
import { HeadersEditor } from './HeadersEditor';
import { ExpectEditor } from './ExpectEditor';
import { RawEditor, ExtractsView } from './SectionViews';
import { OptionsEditor } from './OptionsEditor';
import { TlsEditor } from './TlsEditor';
import { MetaEditor } from './MetaEditor';
import { DatasetEditor } from './DatasetEditor';
import { ProtoEditor } from './ProtoEditor';
import { BenchEditor } from './BenchEditor';
import { PlanView } from './PlanView';
import { ConfigTab } from './ConfigTab';

export function SectionTabs() {
  const tab = useStore(s => s.requestTab);
  const setTab = useStore(s => s.setRequestTab);
  const bodies = useStore(s => s.request.bodies);
  const headers = useStore(s => s.request.headers);
  const parsed = useStore(s => s.collectionParsed);
  const problemCount = useStore(s => s.problemCount);
  const family = useStore(s => requestFamily(s.workspacePath, s.request.endpoint));

  const groups = sectionsByGroup(parsed, bodies, headers, family);
  const editors = groups.editor;
  const views = groups.view.map(s =>
    s.key === 'source' && problemCount > 0 ? { ...s, count: problemCount } : s,
  );
  const configCount = groups.config.length;
  const configOpen = tab === 'config' || groups.config.some(s => s.key === tab);

  const items: TabItem<RequestTab>[] = [
    ...editors.map(s => ({
      key: s.key,
      label: <>{s.label}{s.count !== undefined && <span className="badge">{s.count}</span>}</>,
    })),
    {
      key: 'config' as RequestTab,
      label: <>config{configCount > 0 && <span className="badge">{configCount}</span>}</>,
    },
    ...views.map(s => ({
      key: s.key,
      label: <>{s.label}{s.count !== undefined && <span className="badge">{s.count}</span>}</>,
    })),
  ];

  return (
    <Tabs
      id="section"
      label="Sections of this request"
      items={items}
      value={configOpen ? ('config' as RequestTab) : tab}
      onChange={setTab}
      className="section-strip"
    />
  );
}

export function SectionBody({ fill }: { fill: boolean }) {
  const tab = useStore(s => s.requestTab);
  const parsed = useStore(s => s.collectionParsed);
  const reason = useStore(s => rawAuthorityReason(s));
  const setRequestTab = useStore(s => s.setRequestTab);
  const bodies = useStore(s => s.request.bodies);
  const headers = useStore(s => s.request.headers);
  const family = useStore(s => requestFamily(s.workspacePath, s.request.endpoint));
  const config = sectionsByGroup(parsed, bodies, headers, family).config;
  const shown = tab === 'config' || config.some(s => s.key === tab) ? 'config' : tab;
  const panel = { ...tabPanelProps('section', shown), className: `section-body${fill ? ' is-fill' : ''}` };

  if (reason !== null && tab !== 'source' && tab !== 'plan') {
    return (
      <div {...panel}>
      <div className="stack">
        <div className="note is-warn">
          {reason === 'unreadable'
            ? 'A section of this file could not be read, so the text is what a save writes — these fields hold only what parsed.'
            : reason === 'no-file'
              ? 'There is no file behind this text yet, so the text is the whole of it — these fields hold only what parsed from it.'
              : 'The source tab has unsaved edits, so it is what a save writes. These fields still show the file as it was loaded.'}
          <button className="btn is-sm is-ghost" onClick={() => setRequestTab('source')}>open source</button>
        </div>
        <SectionFor tab={tab} parsed={parsed} />
      </div>
      </div>
    );
  }

  return <div {...panel}><SectionFor tab={tab} parsed={parsed} /></div>;
}

function SectionFor({ tab, parsed }: { tab: RequestTab; parsed: CollectionParsed | null }) {
  switch (tab) {
    case 'body': return <BodyEditor />;
    case 'headers': return <HeadersEditor />;
    case 'asserts': return <ExpectEditor parsed={parsed} />;
    case 'extracts': return <ExtractsView extracts={parsed?.extracts ?? {}} />;
    case 'proto': return <ProtoEditor />;
    case 'source': return <RawEditor />;
    case 'meta': return <MetaEditor />;
    case 'options': return <OptionsEditor />;
    case 'tls': return <TlsEditor />;
    case 'dataset': return <DatasetEditor />;
    case 'bench': return <BenchEditor />;
    case 'config': return <ConfigTab />;
    case 'plan': return <PlanView />;
    default: return <BodyEditor />;
  }
}
