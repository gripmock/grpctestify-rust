import { chainDiagram, diagramLayout, fit, groupsOf } from '../../lib/chain-diagram';
import type { DiagramModel, StepSummary } from '../../lib/chain-diagram';
import type { DocumentSummary } from '../../lib/types';

const WIDTH = 680;
const CLIENT_X = 104;
const SERVER_X = 566;
const MID = (CLIENT_X + SERVER_X) / 2;

export default function ChainDiagram({ documents, summaries }: {
  documents: DocumentSummary[];
  summaries: StepSummary[];
}) {
  const model: DiagramModel = chainDiagram(documents, summaries);
  const { height, steps } = diagramLayout(model);
  if (model.steps.length === 0) return null;

  return (
    <figure className="chain-diagram">
      <svg
        viewBox={`0 0 ${WIDTH} ${height}`}
        role="img"
        aria-label={`The chain: ${model.steps.map(s => s.request).join(', then ')}`}
      >
        <defs>
          <marker id="cd-arrow" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
            <path d="M0 0 L8 4 L0 8 z" className="cd-head" />
          </marker>
          <marker id="cd-arrow-back" viewBox="0 0 8 8" refX="7" refY="4" markerWidth="7" markerHeight="7" orient="auto">
            <path d="M0 0 L8 4 L0 8 z" className="cd-head is-back" />
          </marker>
        </defs>

        <g className="cd-lane">
          <rect x={CLIENT_X - 62} y={4} width={124} height={22} rx={4} />
          <text x={CLIENT_X} y={19}>workbench</text>
        </g>
        <g className="cd-lane">
          <rect x={SERVER_X - 96} y={4} width={192} height={22} rx={4} />
          <title>{model.server}</title>
          <text x={SERVER_X} y={19}>{fit(model.server || 'the target', 26)}</text>
        </g>
        <line className="cd-life" x1={CLIENT_X} y1={26} x2={CLIENT_X} y2={height - 8} />
        <line className="cd-life" x1={SERVER_X} y1={26} x2={SERVER_X} y2={height - 8} />

        {groupsOf(model.steps).map(group => {
          const top = steps[group.start].y;
          const bottom = steps[group.end].y + steps[group.end].height;
          return (
            <g className="cd-group" key={`g${group.start}`}>
              <rect x={CLIENT_X - 54} y={top - 2} width={SERVER_X - CLIENT_X + 80} height={bottom - top} rx={6} />
              <text x={CLIENT_X - 46} y={top + 10}>at the same time</text>
            </g>
          );
        })}

        {model.steps.map((step, i) => {
          const at = steps[i];
          return (
            <g key={step.index}>
              {step.index % 2 === 0 && !step.parallel && (
                <rect className="cd-band" x={0} y={at.y} width={WIDTH} height={at.height} />
              )}
              <text className="cd-step" x={CLIENT_X - 70} y={at.request + 4}>{step.index}</text>

              <text className="cd-label" x={MID} y={at.request - 7}>
                <title>{step.request}</title>
                {fit(step.request)}
              </text>
              <line
                className="cd-out"
                x1={CLIENT_X}
                y1={at.request}
                x2={SERVER_X}
                y2={at.request}
                markerEnd="url(#cd-arrow)"
              />
              {step.streaming && (
                <line className="cd-out is-stream" x1={CLIENT_X} y1={at.request + 4} x2={SERVER_X - 6} y2={at.request + 4} />
              )}

              <text className="cd-label is-back" x={MID} y={at.response - 7}>{step.response}</text>
              <line
                className="cd-in"
                x1={SERVER_X}
                y1={at.response}
                x2={CLIENT_X}
                y2={at.response}
                markerEnd="url(#cd-arrow-back)"
              />

              {at.note !== null && (
                <g className="cd-note">
                  <rect x={MID - 88} y={at.note - 12} width={176} height={18} rx={9} />
                  <text x={MID} y={at.note + 1}>binds {fit(step.binds.join(', '), 24)}</text>
                </g>
              )}
            </g>
          );
        })}
      </svg>
    </figure>
  );
}
