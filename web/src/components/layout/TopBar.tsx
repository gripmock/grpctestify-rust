import { useState, useRef, useEffect, useMemo, useCallback } from 'react';
import { projectCallEnv, projectEnvNames as callVariableNames, resolveProjectAddress, useStore } from '../../lib/store';
import { effectiveEnvironment, substituteEnv } from '../../lib/env';
import { FlaskConical, Check, Settings, FolderGit2, Globe, ChevronDown, TriangleAlert } from 'lucide-react';
import { ConnectionPopover } from './ConnectionPopover';
import { ThemePicker } from './ThemePicker';
import { addressDecision, addressPlaceholder, chainAddressAt, checkAddress, effectiveAddress } from '../../lib/address';
import { maskValue } from '../../lib/secret-names';
import { useDismiss } from 'luvo/input/useDismiss';
import { useMenuKeys } from 'luvo/input/useMenuKeys';
import { Popover } from 'luvo/ui/Popover';
import { EnvironmentManager } from '../request/EnvironmentManager';
import { durationLabel } from '../../lib/format';
import { defaultAddressFor } from '../../lib/types';
import { requestFamily } from '../../lib/http-endpoint';
import { count } from 'luvo/data/plural';

export function TopBar() {
  const address = useStore(s => s.address);
  const protocol = useStore(s => s.protocol);
  const setAddress = useStore(s => s.setAddress);
  const serverHealthy = useStore(s => s.serverHealthy);
  const environments = useStore(s => s.environments);
  const envManager = useStore(s => s.envManager);
  const openEnvManager = useStore(s => s.openEnvManager);
  const closeEnvManager = useStore(s => s.closeEnvManager);
  const activeEnvironment = useStore(s => s.activeEnvironment);
  const setActiveEnvironment = useStore(s => s.setActiveEnvironment);

  const projectRoot = useStore(s => s.projectRoot);
  const run = useStore(s => s.run);
  const runJobId = useStore(s => s.runJobId);
  const workspaceName = useStore(s => s.workspaceName);
  const showSidebarTab = useStore(s => s.showSidebarTab);
  const ranSomething = run.done > 0 || run.finished;
  const running = runJobId !== null && !run.finished;
  const recentAddresses = useStore(s => s.recentAddresses);
  const projectEnvNames = useStore(s => s.projectEnvNames);
  const [showRecent, setShowRecent] = useState(false);
  const recentRef = useDismiss<HTMLDivElement>(showRecent, useCallback(() => setShowRecent(false), []));
  const family = useStore(s => requestFamily(s.workspacePath, s.request.endpoint));
  const verdict = useMemo(() => checkAddress(address, family), [address, family]);
  const impliedScheme = family === 'httf'
    && address.trim() !== ''
    && !address.includes('://')
    && !address.includes('{{');
  const fileAddress = useStore(s => s.collectionParsed?.address || chainAddressAt(s.documents, s.activeStep) || null);
  const inheritedAddress = useStore(s => !s.collectionParsed?.address && chainAddressAt(s.documents, s.activeStep) !== '');
  const effective = useMemo(() => effectiveAddress(address, fileAddress), [address, fileAddress]);
  const serverAddress = useStore(s => s.serverEnv.address ?? null);
  const environmentAddress = useStore(s => (s.activeEnvironment
    ? s.environments.find(e => e.name === s.activeEnvironment)?.address ?? null
    : null));
  const decision = useMemo(
    () => addressDecision({
      file: fileAddress,
      typed: address,
      environment: environmentAddress,
      server: serverAddress,
      fallback: family === 'httf' ? '' : defaultAddressFor(protocol),
    }),
    [fileAddress, address, environmentAddress, serverAddress, protocol, family],
  );
  const [showDropdown, setShowDropdown] = useState(false);
  const dropdownRef = useRef<HTMLDivElement>(null);
  const closeDropdown = useCallback(() => setShowDropdown(false), []);
  const [envMenuRef, onEnvMenuKeys] = useMenuKeys<HTMLDivElement>(showDropdown, closeDropdown);

  const activeEnv = environments.find(e => e.name === activeEnvironment);

  const resolvedAddress = useMemo(
    () => substituteEnv(address, effectiveEnvironment(activeEnv)),
    [address, activeEnv],
  );
  const hasVarPattern = address?.includes('{{') ?? false;
  const projectEnv = useStore(projectCallEnv);
  const projectEnvName = projectEnv?.name ?? '';
  const fileResolved = resolveProjectAddress(effective.address, projectEnv);
  const callNames = useStore(callVariableNames);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (dropdownRef.current && !dropdownRef.current.contains(e.target as Node)) {
        setShowDropdown(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, []);

  return (
    <header className="topbar">
      <div className="topbar-ident">
        <span className="brand">
          <FlaskConical size={16} className="brand-mark" />
          grpctestify
        </span>
        <span className={`badge ${serverHealthy ? 'is-ok' : 'is-fail'}`} title={serverHealthy ? 'Server reachable' : 'Server unreachable'}>
          <span className={`dot ${serverHealthy ? 'is-ok' : 'is-fail'}`} /> play
        </span>
        {projectRoot && (
          <span className="badge is-info" title="Project mode">
            <FolderGit2 size={10} /> .grpctestify
          </span>
        )}
        {workspaceName !== '' && (
          <span className="mono muted topbar-workspace" title="The directory this workbench is serving">
            {workspaceName}/
          </span>
        )}
      </div>

      <div className="topbar-mid">
      <div ref={dropdownRef} className="picker topbar-env">
        <button
          className="btn is-sm is-quiet picker-trigger"
          onClick={() => setShowDropdown(v => !v)}
          aria-haspopup="menu"
          aria-expanded={showDropdown}
          title={activeEnv
            ? [
              `${activeEnv.name}: ${Object.keys(activeEnv.variables).length} variable(s)`,
              activeEnv.address ? `sends calls to ${activeEnv.address}` : 'uses the address in the header',
            ].join('\n')
            : 'No environment — {{KEY}} is sent as written'}
        >
          <span className="env-mark" aria-hidden="true">◆</span>
          <span className={`picker-value${activeEnvironment ? '' : ' muted'}`}>
            {activeEnvironment ?? 'No environment'}
          </span>
          <ChevronDown size={12} className="no-shrink" />
        </button>

        <Popover open={showDropdown} anchor={dropdownRef} className="env-menu">
          <div ref={envMenuRef} className="menu" role="menu" aria-label="Environment" onKeyDown={onEnvMenuKeys}>
            <button
              className={`menu-item${!activeEnvironment ? ' is-on' : ''}`}
              role="menuitemradio"
              tabIndex={-1}
              aria-checked={!activeEnvironment}
              onClick={() => { setActiveEnvironment(null); setShowDropdown(false); }}
            >
              <span className="env-check">{!activeEnvironment && <Check size={12} />}</span>
              <span className="grow">No environment</span>
              <span className="muted">
                {callNames.length === 0
                  ? <>{'{{KEY}}'} stays as written</>
                  : <>{'{{KEY}}'} stays as written — bar the {callNames.length} in “{projectEnvName}”</>}
              </span>
            </button>
            {environments.map(env => {
              const isActive = activeEnvironment === env.name;
              const fromProject = projectEnvNames.includes(env.name);
              const varCount = Object.keys(env.variables).length;
              return (
                <button
                  key={env.name}
                  className={`menu-item${isActive ? ' is-on' : ''}`}
                  role="menuitemradio"
                  tabIndex={-1}
                  aria-checked={isActive}
                  onClick={() => { setActiveEnvironment(env.name); setShowDropdown(false); }}
                  title={fromProject
                    ? `From .grpctestify/.env.${env.name} and .env.${env.name}.local`
                    : 'Kept in this browser only'}
                >
                  <span className="env-check">{isActive && <Check size={12} />}</span>
                  <span className="row-name env-menu-name">{env.name}</span>
                  <span className="badge is-kind">
                    {fromProject ? <FolderGit2 size={10} /> : <Globe size={10} />}
                    {fromProject ? 'file' : 'browser'}
                  </span>
                  <span className="grow" />
                  {env.address && <span className="mono muted env-menu-target">→ {env.address}</span>}
                  <span className="muted">{count(varCount, 'var')}</span>
                </button>
              );
            })}
            <div className="menu-sep" />
            <button
              className="menu-item"
              role="menuitem"
              tabIndex={-1}
              onClick={() => { setShowDropdown(false); openEnvManager(); }}
            >
              <Settings size={12} /> Manage environments…
            </button>
          </div>
        </Popover>
      </div>

      <div className="topbar-conn">
      <div
        ref={recentRef}
        className={`field-frame grow${hasVarPattern && activeEnv ? ' is-templated' : ''}${verdict.ok ? '' : ' is-warn'}`}
      >
        {impliedScheme && <span className="mono muted addr-scheme" title="An address with no scheme is dialled over http://">http://</span>}
        <input
          className="field mono"
          value={address}
          onChange={e => setAddress(e.target.value)}
          onFocus={() => { if (address.trim() === '') setShowRecent(recentAddresses.length > 0); }}
          placeholder={addressPlaceholder({ file: fileAddress, environment: environmentAddress, server: serverAddress, protocol, family })}
          spellCheck={false}
          title={
            address.trim() === '' || decision.source !== 'typed'
              ? decision.address
                ? `Calls go to ${decision.address} — ${decision.why}`
                : decision.why.charAt(0).toUpperCase() + decision.why.slice(1)
              : hasVarPattern && activeEnv?.name
              ? `Resolves to: ${resolvedAddress}\n\nVariables:\n${
                  Object.entries(activeEnv.variables)
                    .filter(([k]) => address.includes(`{{${k}}}`))
                    .map(([k, v]) => `  ${k}=${v ? maskValue(k, v, activeEnv.secret) : '(secret)'}`)
                    .join('\n')
                }`
                : activeEnv?.name
                  ? `Active env: "${activeEnv.name}" — {{KEY}} will be substituted`
                  : 'Use {{KEY}} patterns with an active environment'
          }
        />
        {hasVarPattern && activeEnv && resolvedAddress !== address && (
          <span className="badge is-info mono" title={resolvedAddress}>{resolvedAddress}</span>
        )}
        {effective.overridden && (
          <button
            className="badge is-pending mono addr-file"
            onClick={() => setAddress(effective.address)}
            title={`${inheritedAddress
              ? `This step dials ${fileResolved} — the address the chain started with.`
              : `This file dials ${fileResolved} — its ADDRESS section wins over the field.`}${
              fileResolved === effective.address
                ? ''
                : `\nThe file says ${effective.address}; the project's "${projectEnvName}" environment resolves it where the call is made.`
            } Click to type it here.`}
          >
            {inheritedAddress ? 'chain: ' : 'file: '}{effective.address}
          </button>
        )}
        {effective.overridden && fileResolved !== effective.address && (
          <span className="badge is-info mono" title={`${effective.address} through "${projectEnvName}"`}>
            {fileResolved}
          </span>
        )}
        {!verdict.ok && address.trim() !== '' && (
          <span className="addr-warn" title={verdict.reason}>
            <TriangleAlert size={11} />
          </span>
        )}
        {verdict.ok && verdict.note !== undefined && (
          <span className="addr-warn is-note" title={verdict.note}>
            <TriangleAlert size={11} />
          </span>
        )}

        {recentAddresses.length > 0 && (
          <button
            className="btn is-sm is-ghost is-icon"
            onClick={() => setShowRecent(v => !v)}
            aria-haspopup="menu"
            aria-expanded={showRecent}
            title="Addresses this browser has dialled"
            aria-label="Addresses this browser has dialled"
          >
            <ChevronDown size={11} />
          </button>
        )}

        {showRecent && recentAddresses.length > 0 && (
          <div className="menu addr-menu">
            <div className="menu-group">recent</div>
            {recentAddresses.map(a => (
              <button
                key={a}
                className={`menu-item${a === address ? ' is-on' : ''}`}
                onMouseDown={e => { e.preventDefault(); setAddress(a); setShowRecent(false); }}
              >
                <span className="mono grow">{a}</span>
              </button>
            ))}
          </div>
        )}
      </div>

      <ConnectionPopover />
      </div>
      </div>

      <div className="topbar-tools">
        {ranSomething && !running && (
          <button
            className="btn is-ghost is-sm topbar-run"
            onClick={() => showSidebarTab('collections')}
            title={`The last run: ${run.passed} passed, ${run.failed} failed${run.skipped > 0 ? `, ${run.skipped} skipped` : ''} — open the rail where the files are`}
          >
            <span className="field-label">run</span>
            {run.passed > 0 && <span className="run-pass">✓ {run.passed}</span>}
            {run.failed > 0 && <span className="run-fail">✗ {run.failed}</span>}
            {run.durationMs > 0 && <span className="muted mono">{durationLabel(run.durationMs)}</span>}
          </button>
        )}
        <ThemePicker />
      </div>

      {envManager && (
        <EnvironmentManager
          defineVar={envManager.defineVar}
          defineValue={envManager.value}
          onClose={closeEnvManager}
        />
      )}
    </header>
  );
}
