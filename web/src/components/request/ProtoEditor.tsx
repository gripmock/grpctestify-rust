import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Seg } from 'luvo/ui/Seg';
import { useStore, workspaceDirty } from '../../lib/store';
import { PROTO_SOURCES, applyProtoSource, csvJoin, csvList, protoSourceOf, setKey } from '../../lib/section-model';
import type { ProtoSource } from '../../lib/section-model';
import { bytesToBase64, protoKindOf, type ProtoFile, type ProtoKind } from '../../lib/proto-files';
import { missingPaths } from '../../lib/assert-problems';
import { useToast } from 'luvo/ui/ToastContext';
import { useModal } from 'luvo/ui/ModalContext';
import { fromFileRelative, relativeToFile } from '../../lib/relative-path';
import { pathPlaceholderNote } from '../../lib/path-placeholder';
import { offerNote } from '../../lib/proto-offer';
import { referencedNote } from '../../lib/delete-warning';
import { apiPath } from '../../lib/api-path';
import { humanBytes } from '../../lib/format';
import { Upload, X } from 'lucide-react';
import { count } from 'luvo/data/plural';

const LABEL: Record<string, string> = {
  reflection: 'reflection',
  descriptor: 'descriptor',
  files: '.proto files',
};

export function ProtoEditor() {
  const parsed = useStore(s => s.collectionParsed);
  const setSectionKv = useStore(s => s.setSectionKv);
  const reflectStatus = useStore(s => s.reflectStatus);
  const reflectError = useStore(s => s.reflectError);
  const methodCount = useStore(s => s.reflectionMethods.length);
  const reflect = useStore(s => s.reflect);
  const address = useStore(s => s.address);
  const dirty = useStore(workspaceDirty);
  const [draft, setDraft] = useState<Record<string, string>>({ files: '', import_paths: '' });
  const refreshCollections = useStore(s => s.refreshCollections);
  const toast = useToast();
  const modal = useModal();

  const proto = parsed?.proto ?? {};
  const [chosen, setChosen] = useState<ProtoSource | null>(null);
  const workspacePath = useStore(s => s.workspacePath);
  useEffect(() => { setChosen(null); }, [workspacePath]);
  const written = protoSourceOf(proto);
  const source = written !== 'reflection' ? written : (chosen ?? 'reflection');

  const [available, setAvailable] = useState<ProtoFile[]>([]);
  const load = useCallback(() => {
    fetch('/api/proto-files')
      .then(r => (r.ok ? r.json() : []))
      .then((files: ProtoFile[]) => setAvailable(files))
      .catch(() => setAvailable([]));
  }, []);
  useEffect(() => { load(); }, [load]);

  const pickerRef = useRef<HTMLInputElement>(null);
  const [uploading, setUploading] = useState(false);

  const upload = async (file: File) => {
    const kind = protoKindOf(file.name);
    if (!kind) { toast.error(`${file.name}: not a .proto or a descriptor set`); return; }
    setUploading(true);
    try {
      const body = kind === 'descriptor'
        ? {
            filename: file.name,
            encoding: 'base64',
            content: bytesToBase64(new Uint8Array(await file.arrayBuffer())),
          }
        : { filename: file.name, content: await file.text() };
      const res = await fetch('/api/proto-upload', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!res.ok) { toast.error(`${file.name}: ${await res.text()}`); return; }
      load();
      refreshCollections();
      if (kind === 'descriptor') {
        setSectionKv('proto', setKey(applyProtoSource(proto, 'descriptor'), 'descriptor', spell(file.name)));
      } else {
        const next = csvJoin([...csvList(proto.files), spell(file.name)]);
        setSectionKv('proto', setKey(applyProtoSource(proto, 'files'), 'files', next));
      }
      toast.success(`${file.name} added`);
    } finally {
      setUploading(false);
    }
  };

  const removeSchema = async (path: string) => {
    const named = await fetch(`/api/references/${apiPath(path)}`)
      .then(r => (r.ok ? r.json() as Promise<string[]> : []))
      .catch(() => [] as string[]);
    const ok = await modal.confirm(
      'Remove schema',
      [`Delete ${path} from the project?`, referencedNote(named)].filter(Boolean).join(' '),
      { confirmText: 'Delete', cancelText: 'Cancel', danger: true },
    );
    if (!ok) return;
    const res = await fetch(`/api/collections/${apiPath(path)}`, { method: 'DELETE' }).catch(() => null);
    if (!res || !res.ok) { toast.error(`${path} could not be deleted`); return; }
    load();
    refreshCollections();
    toast.success(`${path} deleted`);
  };

  const offer = (kind: ProtoKind) => available.filter(f => f.kind === kind);
  const pathNote = pathPlaceholderNote(proto.files, proto.import_paths, proto.descriptor);
  const diagnostics = useStore(s => s.diagnostics);
  const missing = useMemo(() => missingPaths(diagnostics, 'PROTO'), [diagnostics]);
  const spell = (listedPath: string) => relativeToFile(workspacePath, listedPath);
  const listed = (spelled: string) => fromFileRelative(workspacePath, spelled);

  const addTo = (key: 'files' | 'import_paths') => {
    const value = draft[key].trim();
    if (!value) return;
    const next = csvJoin([...csvList(proto[key]), value]);
    setSectionKv('proto', setKey(proto, key, next));
    setDraft(d => ({ ...d, [key]: '' }));
  };

  const removeFrom = (key: 'files' | 'import_paths', item: string) => {
    const next = csvJoin(csvList(proto[key]).filter(x => x !== item));
    setSectionKv('proto', setKey(proto, key, next));
  };

  return (
    <div className="stack">
      <div className="bar">
        <span className="label">schema from</span>
        <Seg
          label="Where the schema comes from"
          value={source}
          onChange={s => { setChosen(s); setSectionKv('proto', applyProtoSource(proto, s)); }}
          options={PROTO_SOURCES.map(s => ({ value: s, label: LABEL[s] }))}
        />
        <span className="grow" />
        <span className="muted">
          {reflectStatus === 'loading' ? 'reading the schema…'
            : reflectError ? reflectError
            : methodCount > 0 ? count(methodCount, 'method')
            : source === 'reflection' ? 'the server is asked at run time'
            : 'no methods loaded yet'}
        </span>
        <button
          className="btn is-sm is-ghost"
          disabled={reflectStatus === 'loading' || dirty || !address}
          onClick={() => void reflect()}
          title={
            !address ? 'Set an address first — the schema is read over it'
            : dirty ? 'Save the file first — the schema is read from the file on disk'
            : 'Read the methods this schema declares'
          }
        >
          load methods
        </button>
      </div>

      {source === 'reflection' && (
        <div className="note">
          No PROTO section is written. The address must serve the reflection API — descriptor and
          <span className="mono"> .proto</span> files are peers of this, not fallbacks discovered
          after a hang.
        </div>
      )}

      {source === 'descriptor' && pathNote && <div className="note is-warn">{pathNote}</div>}

      {missing.map(({ named, at }, i) => (
        <div key={`missing-${i}`} className="note is-warn">
          <span className="mono">{named}</span> is not there
          {at !== null && <> — the workbench looked in <span className="mono">{at}</span></>}.
        </div>
      ))}

      {source === 'descriptor' && (
        <div className="stack">
          <label className="stack">
            <span className="label">descriptor set</span>
            <input className="field field-frame mono" placeholder="path to the descriptor"
              value={proto.descriptor ?? ''}
              onChange={e => setSectionKv('proto', setKey(proto, 'descriptor', e.target.value))} />
          </label>
          <ProtoOffer
            files={offer('descriptor')}
            active={name => listed(proto.descriptor ?? '') === name}
            onRemove={path => void removeSchema(path)}
            onPick={name => setSectionKv('proto', setKey(proto, 'descriptor', spell(name)))}
            empty={offerNote('descriptor set', (proto.descriptor ?? '').trim() !== '')}
          />
          <div className="note">
            A compiled set (<span className="mono">protoc -o</span>,{' '}
            <span className="mono">buf build -o</span>) needs no imports and no reflection.
          </div>
        </div>
      )}

      {source === 'files' && (['files', 'import_paths'] as const).map(key => (
        <div key={key} className="stack">
          <span className="label">{key === 'files' ? '.proto files' : 'import paths'}</span>
          <div className="bar wrap">
            {csvList(proto[key]).map(item => (
              <span key={item} className="chip is-on mono">
                {item}
                <button className="btn is-ghost is-icon" aria-label={`Remove ${item}`} onClick={() => removeFrom(key, item)}>
                  <X size={9} />
                </button>
              </span>
            ))}
          </div>
          <div className="field-frame">
            <input
              className="field mono"
              placeholder={key === 'files' ? './proto/auth.proto' : './proto'}
              value={draft[key]}
              onChange={e => setDraft(d => ({ ...d, [key]: e.target.value }))}
              onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); addTo(key); } }}
            />
            <button className="btn is-ghost is-sm" onClick={() => addTo(key)} disabled={!draft[key].trim()}>add</button>
          </div>
        </div>
      ))}

      {source === 'files' && pathNote && <div className="note is-warn">{pathNote}</div>}

      {source === 'files' && (
        <ProtoOffer
          files={offer('proto')}
          active={name => csvList(proto.files).map(listed).includes(name)}
          onRemove={path => void removeSchema(path)}
          onPick={name => setSectionKv('proto', setKey(proto, 'files', csvJoin([...csvList(proto.files), spell(name)])))}
          empty={offerNote('.proto', csvList(proto.files).length > 0)}
        />
      )}

      {source === 'files' && (
        <div className="note">
          Paths are stored comma-separated and resolve relative to this file. Sources that import
          other modules need those too — <span className="mono">buf export &lt;source&gt;
          --output=proto/</span> writes a module and its dependencies into one directory.
        </div>
      )}

      {source !== 'reflection' && (
        <div className="bar">
          <input
            ref={pickerRef}
            type="file"
            accept=".proto,.pb,.bin,.desc,.protoset"
            className="is-hidden"
            onChange={e => {
              const file = e.target.files?.[0];
              e.target.value = '';
              if (file) void upload(file);
            }}
          />
          <button
            className="btn is-sm is-ghost"
            onClick={() => pickerRef.current?.click()}
            disabled={uploading}
            title="Copy a .proto or a compiled descriptor set into the collections"
          >
            <Upload size={11} /> {uploading ? 'uploading…' : 'upload a schema'}
          </button>
        </div>
      )}
    </div>
  );
}

function ProtoOffer({ files, active, onPick, onRemove, empty }: {
  files: ProtoFile[];
  active: (name: string) => boolean;
  onPick: (name: string) => void;
  onRemove: (path: string) => void;
  empty: string;
}) {
  if (files.length === 0) return <div className="muted">{empty}</div>;
  return (
    <div className="bar wrap">
      <span className="label">in this project</span>
      {files.map(f => (
        <span key={f.path} className={`chip mono${active(f.path) ? ' is-on' : ''}`}>
          <button className="chip-name" onClick={() => onPick(f.path)} title={`${f.path} · ${humanBytes(f.size)}`}>
            {f.path}
          </button>
          <button
            className="btn is-ghost is-icon"
            aria-label={`Remove ${f.path}`}
            title={`Remove ${f.path} from the project`}
            onClick={() => onRemove(f.path)}
          >
            <X size={9} />
          </button>
        </span>
      ))}
    </div>
  );
}
