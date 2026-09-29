import { Component, type ReactNode } from 'react';
import { isStaleChunkError } from '../../lib/build-id';
import { resetViewState } from '../../lib/crash-reset';

interface State {
  error: Error | null;
}

export class Crash extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;

    const updated = isStaleChunkError(error.message);

    return (
      <div className="crash">
        <div className="stack crash-card">
          <h1 className="crash-title">
            {updated ? 'The workbench was updated while this tab was open.' : 'The workbench stopped drawing.'}
          </h1>
          <p className="muted">
            {updated
              ? 'This tab is running the build it opened with, and part of it is no longer on the server. Reloading picks up the new one; the tabs come back with it.'
              : 'Your files are on disk and untouched. Reloading rebuilds the window; the tabs come back with it. Resetting the window drops the tabs and everything the window remembers about itself — pane sizes, the drawer, the run scope — and keeps the history, the environments and the project settings.'}
          </p>
          <pre className="crash-what mono">{error.message}</pre>
          <div className="bar">
            <button className="btn" onClick={() => window.location.reload()}>reload</button>
            <button
              className="btn is-ghost"
              onClick={() => {
                resetViewState();
                window.location.reload();
              }}
            >
              reload with the window reset
            </button>
          </div>
        </div>
      </div>
    );
  }
}
