import { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { bindingsOf, callAddress, contentUnread, fileMissing, formsAheadOfFile, projectEnvNames, useStore, workspaceDirty } from '../../lib/store';
import { runWithGate, type RunGateChoice } from '../../lib/run-gate';
import { groupMethods, matchesQuery } from '../../lib/method-search';
import { useIntentFlag } from '../../lib/use-intent';
import { useToast } from 'luvo/ui/useToast';
import { copyToClipboard } from 'luvo/data/clipboard';
import { useModal } from 'luvo/ui/useModal';
import { Popover } from 'luvo/ui/Popover';
import { useDismiss } from 'luvo/input/useDismiss';
import { SectionTabs, SectionBody } from './SectionTabs';
import { SaveDialog } from './SaveDialog';
import { SHAPE_LABEL, SHAPE_SOURCE_NOTE, SHAPE_TONE, SHAPE_ARROW, shapeOfMethod, shapeOfRequest, shapeSource } from '../../lib/shape';
import { ProblemsRow } from './ProblemsRow';
import { SchemaSource } from './SchemaSource';
import { schemaKey, shouldAskServer } from '../../lib/reflect-outcome';
import { METHODS, httpUrl, isHttpRequest, joinEndpoint, methodTone, noHostYet, pathIssue, splitEndpoint } from '../../lib/http-endpoint';
import { splitUrl } from '../../lib/curl-import';
import { methodProblem } from '../../lib/assert-problems';
import { importable } from '../../lib/import-command';
import { joinPath, splitPath, type QueryParam } from '../../lib/query';
import { PairRows } from './PairRows';
import { effectiveEnvironment, substituteEnv } from '../../lib/env';
import { envUsage } from '../../lib/env-usage';
import { columnsOf } from '../../lib/dataset-model';
import { clampRow, rowLabel, rowValues, rowsOf } from '../../lib/dataset-row';
import { Play, Save, Square, ChevronDown, Loader2, ListChecks, RefreshCw, FilePlus2, Upload, Undo2 } from 'lucide-react';
import { count, plural } from 'luvo/data/plural';

export function RequestPanel() {
  const request = useStore(s => s.request);
  const setEndpoint = useStore(s => s.setEndpoint);
  const setAddress = useStore(s => s.setAddress);
  const execute = useStore(s => s.execute);
  const cancel = useStore(s => s.cancel);
  const runTest = useStore(s => s.runTest);
  const ahead = useStore(formsAheadOfFile);
  const runStatus = useStore(s => s.runStatus);
  const runMode = useStore(s => s.runMode);
  const setRunMode = useStore(s => s.setRunMode);
  const reflectionMethods = useStore(s => s.reflectionMethods);
  const address = useStore(s => s.address);

  const saveWorkspace = useStore(s => s.saveWorkspace);
  const saveWorkspaceAs = useStore(s => s.saveWorkspaceAs);
  const workspacePath = useStore(s => s.workspacePath);
  const toast = useToast();
  const modal = useModal();

  const saveIntent = useStore(s => s.saveIntent);
  const saveAsIntent = useStore(s => s.saveAsIntent);
  const reflectStatus = useStore(s => s.reflectStatus);
  const reflect = useStore(s => s.reflect);
  const [saving, setSaving] = useState(false);
  const [showSaveDialog, setShowSaveDialog] = useIntentFlag(saveAsIntent);
  const pickIntent = useStore(s => s.pickIntent);
  const [showDropdown, setShowDropdown] = useIntentFlag(pickIntent);
  const [cursor, setCursor] = useState(0);
  const [dropdownSearch, setDropdownSearch] = useState('');
  const [scaffolding, setScaffolding] = useState(false);
  const scaffoldTest = useStore(s => s.scaffoldTest);
  const [focusDropdownSearch, setFocusDropdownSearch] = useIntentFlag(pickIntent);
  const [showModeMenu, setShowModeMenu] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const modeMenuRef = useRef<HTMLDivElement>(null);
  const pickerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const requestTab = useStore(s => s.requestTab);
  const activeStep = useStore(s => s.activeStep);
  const stepCount = useStore(s => s.documents.length);
  const responseMessages = useStore(s => s.response?.messages.length ?? 0);
  const reportedShape = useStore(s => s.response?.shape ?? null);
  const shape = shapeOfRequest(request.endpoint, request.bodies.length, reflectionMethods, responseMessages, reportedShape);
  const shapeFrom = shapeSource(request.endpoint, reflectionMethods, reportedShape);
  const isExecuting = useStore(s => s.response?.status) === 'pending';
  const canExecute = !!request.endpoint && !isExecuting;
  const dirty = useStore(workspaceDirty);
  const missing = useStore(fileMissing);

  const reflectedAddress = useStore(s => s.reflectedAddress);
  const protocol = useStore(s => s.protocol);
  const dialled = useStore(callAddress);
  const unread = useStore(contentUnread);
  const stale = useStore(s => s.staleOnDisk);
  const handleEndpointFocus = useCallback(() => {
    const key = schemaKey({ address: dialled, protocol, collectionPath: workspacePath });
    if (shouldAskServer({ address: dialled, askedFor: reflectedAddress, status: reflectStatus, key })) reflect();
  }, [dialled, protocol, workspacePath, reflectedAddress, reflectStatus, reflect]);

  const grouped = useMemo(() => groupMethods(reflectionMethods), [reflectionMethods]);

  const filteredDropdown = useMemo(() => {
    if (!dropdownSearch) return grouped;
    const q = dropdownSearch;
    return grouped
      .map(([svc, methods]) => [
        svc,
        methods.filter(m => matchesQuery(m.fullName, q)),
      ] as const)
      .filter(entry => entry[1].length > 0);
  }, [grouped, dropdownSearch]);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node) && inputRef.current && !inputRef.current.contains(e.target as Node)) setShowDropdown(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [setShowDropdown]);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (modeMenuRef.current && !modeMenuRef.current.contains(e.target as Node)) setShowModeMenu(false);
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  const handleSelect = (fullName: string) => {
    setEndpoint(fullName);
    setShowDropdown(false);
    setDropdownSearch('');
    setCursor(0);
  };

  const flatMethods = filteredDropdown.flatMap(([, methods]) => methods);
  const atCursor = flatMethods[Math.min(cursor, flatMethods.length - 1)];

  useEffect(() => {
    if (!showDropdown) return;
    dropdownRef.current?.querySelector<HTMLElement>('.menu-item.is-hover')?.scrollIntoView({ block: 'nearest' });
  }, [cursor, showDropdown]);

  const walk = (e: React.KeyboardEvent, delta: number | 'first' | 'last') => {
    if (flatMethods.length === 0) return;
    e.preventDefault();
    setCursor(current => {
      if (delta === 'first') return 0;
      if (delta === 'last') return flatMethods.length - 1;
      const next = current + delta;
      return Math.max(0, Math.min(flatMethods.length - 1, next));
    });
  };

  const onListKeys = (e: React.KeyboardEvent) => {
    switch (e.key) {
      case 'ArrowDown': return walk(e, 1);
      case 'ArrowUp': return walk(e, -1);
      case 'Home': return walk(e, 'first');
      case 'End': return walk(e, 'last');
      case 'PageDown': return walk(e, 8);
      case 'PageUp': return walk(e, -8);
      case 'Enter':
        if (!atCursor) return;
        e.preventDefault();
        handleSelect(atCursor.fullName);
        return;
      case 'Escape':
        e.preventDefault();
        setShowDropdown(false);
        return;
      default:
    }
  };

  const handleSave = async (): Promise<boolean> => {
    if (!workspacePath) {
      setShowSaveDialog(true);
      return false;
    }
    if (!dirty && !missing) return true;
    if (unread) {
      await useStore.getState().hydrateStaleTabs();
      const reread = !contentUnread(useStore.getState());
      if (!reread) {
        const ok = await modal.confirm(
          `${workspacePath} was never read by this tab`,
          'Saving writes what is on screen — the request, its headers and its body. Anything the file had that is not on screen (RESPONSE, ASSERTS, EXTRACT, OPTIONS, TLS, PROTO) is not in this tab and will not be written. Save anyway?',
        );
        if (!ok) return false;
      }
    }
    setSaving(true);
    try {
      const written = await saveWorkspace();
      if (!written) return false;
      const problems = useStore.getState().problemCount;
      if (problems > 0) {
        toast.warn(`Saved — ${count(problems, 'problem')}, open Problems to see them`);
      }
      return true;
    } catch (err: any) {
      toast.error(err?.message || 'Save failed');
      return false;
    } finally {
      setSaving(false);
    }
  };

  const handleRun = async () => {
    if (!workspacePath) return;
    if (!ahead) { void runTest(); return; }
    const choice = await modal.choose(
      `${workspacePath} has edits that are not in the file`,
      'Run reads the file from disk: the endpoint, the body and the ASSERTS it runs are the saved ones.',
      [
        { label: 'Run the file as saved', value: 'run' },
        { label: 'Save, then run', value: 'save', tone: 'primary' },
      ],
    );
    const outcome = await runWithGate(choice as RunGateChoice, { save: handleSave, run: runTest });
    if (outcome === 'save-refused') toast.error('Not run — the save did not go through');
  };

  const handleDiscard = useCallback(async () => {
    const path = useStore.getState().workspacePath;
    if (!path) return;
    const ok = await modal.confirm(
      `Discard the edits to ${path}?`,
      'The file is read again as it is on disk. This cannot be undone.',
      { confirmText: 'discard', cancelText: 'keep editing', danger: true },
    );
    if (!ok) return;
    if (await useStore.getState().discardEdits()) toast.success('Read again from disk');
    else toast.error('Nothing was discarded — the file could not be read');
  }, [modal, toast]);

  const discardIntent = useStore(s => s.discardIntent);
  const discardRef = useRef(handleDiscard);
  useEffect(() => { discardRef.current = handleDiscard; });
  useEffect(() => {
    if (discardIntent > 0) void discardRef.current();
  }, [discardIntent]);

  const saveRef = useRef(handleSave);
  useEffect(() => { saveRef.current = handleSave; });
  useEffect(() => {
    if (saveIntent > 0) saveRef.current();
  }, [saveIntent]);

  const focusOnPick = useRef(handleEndpointFocus);
  useEffect(() => { focusOnPick.current = handleEndpointFocus; });
  useEffect(() => {
    if (pickIntent > 0) focusOnPick.current();
  }, [pickIntent]);

  const isHttp = isHttpRequest(workspacePath, request.endpoint);
  const noTarget = isHttp && dialled.trim() === '';
  const verb = useMemo(() => splitEndpoint(request.endpoint), [request.endpoint]);
  const pastedCommand = importable(request.endpoint);
  const issue = isHttp && !pastedCommand ? pathIssue(verb.path) : null;
  const wholeUrl = isHttp && !pastedCommand && /^https?:\/\//.test(verb.path.trim());
  const query = useMemo(() => splitPath(verb.path), [verb.path]);
  const [paramsOpen, setParamsOpen] = useState(false);
  const [paramRows, setParamRows] = useState<QueryParam[]>([]);
  const paramsRef = useDismiss<HTMLDivElement>(paramsOpen, useCallback(() => setParamsOpen(false), []));
  const activeEnv = useStore(s => (s.activeEnvironment
    ? s.environments.find(e => e.name === s.activeEnvironment)
    : undefined));
  const resolvedPath = useMemo(
    () => substituteEnv(verb.path, effectiveEnvironment(activeEnv)),
    [verb.path, activeEnv],
  );

  const diagnostics = useStore(s => s.diagnostics);
  const methodSaid = useMemo(
    () => (isHttp ? methodProblem(verb.method, diagnostics) : null),
    [isHttp, verb.method, diagnostics],
  );

  const fullUrl = useMemo(
    () => (isHttp ? httpUrl(dialled, resolvedPath) : ''),
    [isHttp, dialled, resolvedPath],
  );

  const parsedForVars = useStore(s => s.collectionParsed);
  const projectNames = useStore(projectEnvNames);
  const sourceColumns = useStore(s => s.runDataColumns);
  const chainDocs = useStore(s => s.documents);
  const runBound = useStore(bindingsOf);
  const datasetRow = useStore(s => s.datasetRow);
  const setDatasetRow = useStore(s => s.setDatasetRow);
  const rows = useMemo(() => rowsOf(parsedForVars?.dataset), [parsedForVars]);
  const pickedRow = useMemo(
    () => (rows.length > 0 ? rowValues(rows, clampRow(rows, datasetRow)) : null),
    [rows, datasetRow],
  );
  const unresolved = useMemo(() => {
    const text = [request.endpoint, ...Object.values(request.headers), ...request.bodies].join('\n');
    const runtime = {
      datasetColumns: columnsOf(parsedForVars?.dataset ?? []),
      sourceColumns,
      extracted: chainDocs.slice(0, activeStep).flatMap(d => d.produces ?? []),
      runBound,
      projectNames,
      datasetRowValues: runMode === 'run' ? null : pickedRow,
      mode: runMode,
    };
    return envUsage(text, activeEnv, runtime).filter(u => !u.resolved);
  }, [request, parsedForVars, chainDocs, runBound, activeStep, activeEnv, projectNames, runMode, sourceColumns, pickedRow]);

  const fillsPane = requestTab === 'source'
    || (requestTab === 'body' && request.bodies.length > 0);
  const writeParams = (rows: QueryParam[]) => {
    setParamRows(rows);
    setEndpoint(joinEndpoint(verb.method, joinPath(query.path, rows)));
  };

  const RUN_MODES = [
    { mode: 'execute' as const, icon: <Play size={13} />, label: 'Execute', desc: 'Send using the live editor state' },
    { mode: 'run' as const, icon: <ListChecks size={13} />, label: 'Run', desc: `Run the saved ${isHttp ? '.httf' : '.gctf'} file — ASSERTS/EXTRACT included` },
  ];

  return (
    <fieldset className="panel">
      <legend>{stepCount > 1 ? `request · step ${activeStep + 1} of ${stepCount}` : 'request'}</legend>
      <div className="panel-body stack">
        <div className="bar">
          {isHttp ? (
            <div className="field-frame grow http-endpoint">
              <input
                className={`field mono http-verb is-${methodTone(verb.method)}${methodSaid ? ' is-bad' : ''}`}
                title={methodSaid ?? undefined}
                style={{ ['--verb-ch' as string]: String(Math.max(3, verb.method.length)) }}
                value={verb.method}
                list="http-methods"
                spellCheck={false}
                aria-label="Method"
                onChange={e => setEndpoint(joinEndpoint(e.target.value, verb.path))}
              />
              <datalist id="http-methods">
                {METHODS.map(m => <option key={m} value={m} />)}
              </datalist>
              <input
                className="field mono"
                value={verb.path}
                placeholder="path"
                spellCheck={false}
                aria-label="Path"
                onChange={e => setEndpoint(joinEndpoint(verb.method, e.target.value))}
              />
              {resolvedPath !== verb.path && (
                <span className="badge is-info mono http-resolved" title={`Resolves to ${resolvedPath}`}>
                  {resolvedPath}
                </span>
              )}
              {noHostYet(isHttp, dialled, verb.path) && (
                <span className="warn http-issue">
                  no host yet — name one in the header above, or type a whole url here
                </span>
              )}
              {fullUrl !== '' && fullUrl !== verb.path && (
                <button
                  className="btn is-ghost is-sm mono http-url"
                  title={`This call goes to ${fullUrl} — click to copy it`}
                  onClick={() => {
                    void copyToClipboard(fullUrl)
                      .then(() => toast.success('URL copied'))
                      .catch(() => toast.error('The browser refused the clipboard'));
                  }}
                >
                  {fullUrl}
                </button>
              )}
              {issue && <span className="muted http-issue">{issue}</span>}
              {wholeUrl && (
                <button
                  className="btn is-ghost is-sm http-split"
                  onClick={() => {
                    const { address: host, path } = splitUrl(verb.path);
                    if (!host) return;
                    setAddress(host);
                    setEndpoint(joinEndpoint(verb.method, path));
                  }}
                  title={dialled.trim() === ''
                    ? 'Move the host into ADDRESS, leaving the path here — an environment can aim the file that way'
                    : `The ENDPOINT names the whole url, so this file's ADDRESS (${dialled}) is not read. Move the host into it?`}
                >
                  move host to ADDRESS
                </button>
              )}
              <div className="picker" ref={paramsRef}>
                <button
                  className={`btn is-ghost is-sm http-params${query.params.length > 0 ? ' is-on' : ''}`}
                  onClick={() => { setParamRows(query.params); setParamsOpen(v => !v); }}
                  aria-haspopup="menu"
                  aria-expanded={paramsOpen}
                  title={query.params.length > 0
                    ? `${query.params.length} query ${plural(query.params.length, 'parameter')} in this path`
                    : 'Add a query parameter'}
                >
                  params{query.params.length > 0 && <span className="badge">{query.params.length}</span>}
                </button>
                <Popover open={paramsOpen} anchor={paramsRef} align="end">
                  <PairRows
                    noun="parameter"
                    rows={paramRows}
                    empty="No query parameters — they are written into the path."
                    onChange={writeParams}
                  />
                </Popover>
              </div>
            </div>
          ) : (
          <div className="picker grow" ref={pickerRef}>
            <div className="field-frame grow">
              <span
                className={`badge is-kind inset-start ${SHAPE_TONE[shape]}`}
                title={SHAPE_SOURCE_NOTE[shapeFrom]}
              >
                {SHAPE_LABEL[shape]}
              </span>
              <input
                ref={inputRef}
                className="field mono"
                value={request.endpoint}
                onChange={e => { setEndpoint(e.target.value); setFocusDropdownSearch(false); setShowDropdown(true); setDropdownSearch(e.target.value); setCursor(0); }}
                onKeyDown={e => { if (showDropdown) onListKeys(e); }}
                onFocus={() => { setFocusDropdownSearch(false); setShowDropdown(true); handleEndpointFocus(); }}
                placeholder={workspacePath ? 'package.Service/Method' : 'package.Service/Method — or GET /v1/users'}
                spellCheck={false}
              />
              <button
                className="btn is-ghost is-icon"
                onClick={() => { setFocusDropdownSearch(true); setShowDropdown(!showDropdown); handleEndpointFocus(); }}
                title="Select method"
                aria-label="Select method"
                aria-haspopup="menu"
                aria-expanded={showDropdown}
              >
                <ChevronDown size={14} />
              </button>
            </div>

            <Popover open={showDropdown} anchor={pickerRef} matchWidth className="method-menu">
              <div ref={dropdownRef} className="menu">
                <div className="bar method-menu-head">
                  <input
                    className="field grow"
                    value={dropdownSearch}
                    onChange={e => { setDropdownSearch(e.target.value); setCursor(0); }}
                    onKeyDown={onListKeys}
                    placeholder="Search package, service or method…"
                    autoFocus={focusDropdownSearch}
                  />
                  <button
                    className="btn is-sm is-ghost is-icon"
                    onClick={reflect}
                    disabled={reflectStatus === 'loading' || !address}
                    title={reflectStatus === 'error' ? 'Reflection failed — retry'
                      : reflectionMethods.length === 0 ? 'Ask the server what it serves'
                      : 'Refresh methods from the server'}
                  >
                    {reflectStatus === 'loading' ? <Loader2 size={12} className="animate-spin" /> : <RefreshCw size={12} />}
                  </button>
                </div>

                <SchemaSource onConfigure={() => { setShowDropdown(false); useStore.getState().setRequestTab('config'); }} />

                {filteredDropdown.length === 0 && reflectionMethods.length > 0 && (
                  <div className="empty-state">No methods match</div>
                )}

                {filteredDropdown.map(([svc, methods]) => (
                  <div key={svc}>
                    <div className="menu-group">{svc}</div>
                    {methods.map(m => (
                      <button
                        key={m.fullName}
                        className={`menu-item mono${atCursor?.fullName === m.fullName ? ' is-hover' : ''}`}
                        onClick={() => handleSelect(m.fullName)}
                      >
                        <span className={`badge is-kind ${SHAPE_TONE[shapeOfMethod(m)]}`}>
                          {SHAPE_ARROW[shapeOfMethod(m)]} {SHAPE_LABEL[shapeOfMethod(m)]}
                        </span>
                        {m.name}
                      </button>
                    ))}
                  </div>
                ))}

                {dropdownSearch.trim() !== '' && filteredDropdown.length > 0 && (
                  <div className="menu-foot">
                    {filteredDropdown.reduce((n, [, m]) => n + m.length, 0)} of {reflectionMethods.length} methods
                  </div>
                )}

                {request.endpoint && !isHttp && (
                  <>
                    <div className="menu-sep" />
                    <button
                      className="menu-item"
                      disabled={scaffolding}
                      title={`Open the file grpctestify scaffold would write for ${request.endpoint}`}
                      onClick={async () => {
                        setShowDropdown(false);
                        setScaffolding(true);
                        try {
                          await scaffoldTest();
                          toast.success('Scaffold opened — Save to name it');
                        } catch (err: any) {
                          toast.error(err?.message || 'Scaffold failed');
                        } finally {
                          setScaffolding(false);
                        }
                      }}
                    >
                      {scaffolding ? <Loader2 size={12} className="animate-spin" /> : <FilePlus2 size={12} />}
                      <span className="grow">scaffold a test for this method</span>
                    </button>
                  </>
                )}
              </div>
            </Popover>
          </div>
          )}

          {
          pastedCommand && (
            <button
              className="btn is-sm"
              onClick={() => useStore.getState().requestImport(request.endpoint.trim())}
              title={`Read this ${pastedCommand} command into the request — address, headers and body`}
            >
              <Upload size={12} /> import this {pastedCommand}
            </button>
          )}

          {rows.length > 0 && runMode !== 'run' && (
            <select
              className="field is-sm dataset-pick mono"
              aria-label="Which DATASET row this call is made with"
              title="A run runs every row; one call is one row"
              value={clampRow(rows, datasetRow)}
              onChange={e => setDatasetRow(Number(e.target.value))}
            >
              {rows.map((_, i) => (
                <option key={i} value={i}>{rowLabel(rows, i)}</option>
              ))}
            </select>
          )}

          {unresolved.length > 0 && runMode !== 'run' && (
            <span
              className="badge is-pending unresolved-vars"
              title={[
                `${unresolved.map(u => u.key).join(', ')} — ${unresolved.length === 1 ? 'this name has' : 'these names have'} no value here, and will be sent as written`,
                unresolved.some(u => u.runOnly && u.from === 'dataset')
                  ? 'A DATASET column is answered by a row, and Execute has none — Run the file to use them.'
                  : '',
                unresolved.some(u => u.runOnly && u.from === 'extract')
                  ? 'An extracted name is answered by the step before this one, which Execute does not run.'
                  : '',
                unresolved.some(u => u.runOnly && u.from === 'source')
                  ? 'A column of the source this run is driven over is answered by a row, and Execute has none — Run to use it.'
                  : '',
              ].filter(Boolean).join('\n')}
            >
              {unresolved.length} unresolved
            </span>
          )}

          <div ref={modeMenuRef} className="picker">
            <div className="btn-split">
              {isExecuting ? (
                <button className="btn is-danger" onClick={cancel}>
                  <Square size={14} /> Cancel
                </button>
              ) : runMode === 'run' ? (
                <button
                  className="btn is-primary"
                  onClick={() => void handleRun()}
                  disabled={!workspacePath || runStatus === 'running'}
                  title={workspacePath
                    ? `Run the saved ${isHttp ? '.httf' : '.gctf'} file (⌘⇧⏎) — ASSERTS/EXTRACT included, same engine \`grpctestify run\` uses`
                    : 'Save this as a collection file first'}
                >
                  {runStatus === 'running' ? <Loader2 size={14} className="animate-spin" /> : <ListChecks size={14} />}
                  Run
                </button>
              ) : (
                <button
                  className="btn is-primary"
                  onClick={execute}
                  disabled={!canExecute || noTarget}
                  title={!request.endpoint
                    ? 'Pick an endpoint first'
                    : noTarget
                      ? 'Name a target first — an HTTP call has no default: an ADDRESS section, the environment, or the field above'
                      : 'Send what the editors hold right now (⌘⏎)'}
                >
                  <Play size={14} /> Execute
                </button>
              )}

              <button
                className="btn is-primary"
                onClick={() => setShowModeMenu(v => !v)}
                disabled={isExecuting}
                title={`Execute or Run — currently ${runMode === 'run' ? 'Run' : 'Execute'}`}
                aria-haspopup="menu"
                aria-expanded={showModeMenu}
              >
                <ChevronDown size={14} />
              </button>
            </div>

            <Popover open={showModeMenu} anchor={modeMenuRef} className="mode-menu">
              <div className="menu">
                {RUN_MODES.map(opt => (
                  <button
                    key={opt.mode}
                    className={`menu-item mode-item${runMode === opt.mode ? ' is-on' : ''}`}
                    onClick={() => { setRunMode(opt.mode); setShowModeMenu(false); }}
                  >
                    <span className="mode-icon">{opt.icon}</span>
                    <span className="stack is-flush">
                      <span className="mode-label">{opt.label}</span>
                      <span className="mode-desc">{opt.desc}</span>
                    </span>
                  </button>
                ))}
              </div>
            </Popover>
          </div>

          {stale && (
            <span
              className="badge is-pending"
              title={`${workspacePath} changed on disk while this tab held unsaved edits — saving will ask which to keep`}
            >
              changed on disk
            </span>
          )}
          {unread && !missing && (
            <button
              className="badge is-pending"
              onClick={() => void useStore.getState().hydrateStaleTabs()}
              title={`${workspacePath} has not been read by this tab — what is on screen is the request only. Click to read it again.`}
            >
              not read
            </button>
          )}
          {workspacePath && dirty && (
            <button
              className="btn is-ghost is-icon"
              onClick={() => void handleDiscard()}
              title={`Discard the edits and read ${workspacePath} again`}
              aria-label="Discard the edits"
            >
              <Undo2 size={14} />
            </button>
          )}
          <button
            className="btn"
            onClick={handleSave}
            disabled={saving || (!!workspacePath && !dirty && !missing)}
            title={
              saving ? 'Writing…'
              : !workspacePath ? 'Choose where this file goes (⌘S)'
              : missing ? 'The file is gone — Save writes it again (⌘S)'
              : dirty ? 'Write the edits to the file (⌘S)'
              : 'Nothing has changed since the last save'
            }
          >
            <Save size={14} /> {saving ? 'Saving…' : (workspacePath ? 'Save' : 'Save As…')}
          </button>
        </div>

        <SectionTabs />

        <SectionBody fill={fillsPane} />

        <ProblemsRow />
      </div>

      {showSaveDialog && <SaveDialog
        onClose={() => setShowSaveDialog(false)}
        onSave={async (path, meta, fmt) => {
          try {
            await saveWorkspaceAs(path, meta, fmt);
          } catch (err: any) {
            toast.error(err?.message || 'Save failed');
          }
        }}
      />}
    </fieldset>
  );
}
