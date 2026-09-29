import { useMemo } from 'react';
import { bindingsOf, projectEnvNames, useStore } from '../../lib/store';
import { envUsage, unresolvedCount, type VarSource } from '../../lib/env-usage';
import { maskValue } from '../../lib/secret-names';
import { columnsOf } from '../../lib/dataset-model';
import { clampRow, rowValues } from '../../lib/dataset-row';

const SOURCE_NOTE: Record<VarSource, string> = {
  env: '',
  dataset: 'comes from a DATASET column',
  source: 'comes from a column of the source this run is driven over',
  extract: 'is extracted by an earlier step',
  run: 'was bound by a step of this file that has already answered',
  project: "comes from the project's active environment, where the call is made",
  unknown: '',
};

export function EnvVarToolbar({ text }: { text: string }) {
  const activeEnvironment = useStore(s => s.activeEnvironment);
  const environments = useStore(s => s.environments);
  const collectionParsed = useStore(s => s.collectionParsed);
  const documents = useStore(s => s.documents);
  const activeStep = useStore(s => s.activeStep);
  const openEnvManager = useStore(s => s.openEnvManager);

  const activeEnv = useMemo(
    () => environments.find(e => e.name === activeEnvironment),
    [environments, activeEnvironment],
  );

  const projectNames = useStore(projectEnvNames);
  const mode = useStore(s => s.runMode);
  const sourceColumns = useStore(s => s.runDataColumns);
  const runBound = useStore(bindingsOf);
  const datasetRow = useStore(s => s.datasetRow);
  const runtime = useMemo(() => ({
    datasetColumns: columnsOf(collectionParsed?.dataset ?? []),
    datasetRowValues: mode === 'run' ? null : rowValues(collectionParsed?.dataset, clampRow(collectionParsed?.dataset, datasetRow)),
    sourceColumns,
    extracted: documents.slice(0, activeStep).flatMap(d => d.produces ?? []),
    runBound,
    projectNames,
    mode,
  }), [collectionParsed, documents, activeStep, runBound, projectNames, mode, sourceColumns, datasetRow]);

  const used = useMemo(() => envUsage(text, activeEnv, runtime), [text, activeEnv, runtime]);

  if (used.length === 0) return null;

  const unresolved = unresolvedCount(used);

  return (
    <div className="env-strip">
      <span className="field-label">vars</span>
      {activeEnv && <span className="mono muted">{activeEnv.name}</span>}
      {used.map(({ key, value, muted, resolved, from, empty, runOnly }) => {
        const title =
          muted ? `"${key}" is muted — it will not be substituted`
          : runOnly ? `"${key}" ${SOURCE_NOTE[from]} — a run answers it, Execute has no ${from === 'extract' ? 'earlier step' : 'row'} and sends the braces as written`
          : from === 'run' ? `"${key}" ${SOURCE_NOTE.run} — the call sends ${maskValue(key, value, activeEnv?.secret)}`
          : from === 'dataset' || from === 'extract' || from === 'project' || from === 'source' ? `"${key}" ${SOURCE_NOTE[from]}`
          : empty ? `"${key}" is defined with no value — the call sends an empty string. Click to give it one.`
          : resolved ? maskValue(key, value, activeEnv?.secret)
          : activeEnv ? `"${key}" is not set in ${activeEnv.name} — click to define it`
          : `"${key}" has no value — click to define it`;
        const className = `env from-${from}${resolved ? '' : ' is-unknown'}${empty ? ' is-empty' : ''}${muted ? ' is-muted' : ''}`;
        if (resolved && !empty) return <span key={key} className={className} title={title}>{key}</span>;
        return (
          <button key={key} type="button" className={className} title={title} onClick={() => openEnvManager(key)}>
            {key}
          </button>
        );
      })}
      <span className="grow" />
      {unresolved > 0 && <span className="env-unresolved">{unresolved} unresolved</span>}
    </div>
  );
}
