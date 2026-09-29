import { useStore } from '../../lib/store';
import { schemaState } from '../../lib/schema-state';
import { protoSourceOf, csvList } from '../../lib/section-model';
import { FileCode2 } from 'lucide-react';

const EMPTY: Record<string, string> = {};

function clock(ms: number): string {
  const at = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, '0');
  return `${pad(at.getHours())}:${pad(at.getMinutes())}`;
}

export function SchemaSource({ onConfigure }: { onConfigure: () => void }) {
  const reflectStatus = useStore(s => s.reflectStatus);
  const reflectError = useStore(s => s.reflectError);
  const methodCount = useStore(s => s.reflectionMethods.length);
  const serviceCount = useStore(s => new Set(s.reflectionMethods.map(m => m.service)).size);
  const parsed = useStore(s => s.collectionParsed);
  const reflectedAt = useStore(s => s.reflectedAt);
  const reflect = useStore(s => s.reflect);
  const proto = parsed?.proto ?? EMPTY;

  const state = schemaState({
    reflectStatus,
    reflectError,
    methodCount,
    serviceCount,
    protoSource: protoSourceOf(proto),
    protoFiles: csvList(proto.files ?? '').length,
    protoNames: proto.descriptor || proto.files || '',
    reflectedAt: reflectedAt === null ? null : clock(reflectedAt),
  });

  return (
    <div className={`schema-source${state.tone === 'fail' ? ' is-fail' : ''}`}>
      <FileCode2 size={11} />
      <span className="grow">
        <span className="mono">{state.label}</span>
        <span className="muted schema-hint">{state.hint}</span>
      </span>
      {state.kind === 'unasked' && (
        <button className="btn is-sm" onClick={() => void reflect()}>
          ask the server
        </button>
      )}
      {state.kind !== 'reflected' && (
        <button className="btn is-sm is-ghost" onClick={onConfigure}>
          PROTO section
        </button>
      )}
    </div>
  );
}
