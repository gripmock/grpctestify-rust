import { useStore } from './store';
import { columnsOf } from './dataset-model';
import type { RuntimeNames } from './env-usage';
import type { Environment } from './types';

export function currentEnv(): Environment | null {
  const st = useStore.getState();
  return st.environments.find(e => e.name === st.activeEnvironment) ?? null;
}

export function currentRuntime(): RuntimeNames {
  const st = useStore.getState();
  return {
    datasetColumns: columnsOf(st.collectionParsed?.dataset ?? []),
    extracted: st.documents.slice(0, st.activeStep).flatMap(d => d.produces ?? []),
  };
}
