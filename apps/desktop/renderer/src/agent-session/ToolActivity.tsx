import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { ExpandIcon, ToolIcon } from '../tool-icon';
import { toolStateIcon, toolTypeIcon } from '../tool-icon-map';
import { BrowserScreenshot } from './BrowserScreenshot';
import { useDisclosureOpen } from './disclosure-open';
import { ModelTerminalLink } from './ModelTerminalLink';
import { ScrollBox } from '../ui/ScrollBox';
import { remoteDecisionText } from './remote-decision';
import { callUnderWay, stateOf, toneOf } from './tool-call-tone';
import { DetailLine, TargetLabel } from './ToolFileLinks';

export function ToolActivity({
  entry,
  ended = false,
}: {
  entry: AgentToolActivityEntry | undefined;
  /** The turn is over: a call with no record by then never ran. */
  ended?: boolean;
}) {
  // A call Core has not recorded yet (its batch not admitted) is shown with its state.
  if (!entry) return <UnrecordedCall ended={ended} />;
  if (entry.display.internal) return null;
  return (
    <>
      {entry.display.details.length > 0 || entry.display.image ? (
        <ExpandableToolActivity entry={entry} />
      ) : (
        <ToolActivityLine entry={entry} />
      )}
      <ModelTerminalLink callId={entry.summary.tool_call_id} />
    </>
  );
}

/** The leading slot spans both lines, so a wrapped brief still starts under the title, and the
 *  tool icon lines up with 思考过程 above it. */
function LeadingSlot({ kind }: { kind: ReturnType<typeof toolTypeIcon> }) {
  return (
    <span className="agent-tool-leading">
      <ToolIcon className="agent-tool-kind" name={kind} />
    </span>
  );
}

/** What a call's screenshot shows, as its picture is named. */
function screenshotName(tool: string): string {
  if (tool.startsWith('computer.')) return '屏幕截图';
  if (tool.startsWith('chrome.')) return 'Chrome 截图';
  return '网页截图';
}

/** `open` is omitted when there is nothing to expand: no dead glyph at the end of the line. */
function ToolActivityBody({ entry, open }: { entry: AgentToolActivityEntry; open?: boolean }) {
  const { outcome, text } = stateOf(entry);
  const tone = toneOf(outcome, entry.summary.status);
  return (
    <span className="agent-tool-main">
      <span className="agent-tool-title">{entry.display.title}</span>
      {entry.display.target ? (
        <TargetLabel
          call={entry.summary.tool_call_id}
          target={entry.display.target}
          tool={entry.summary.display_name}
        />
      ) : null}
      <span className="agent-tool-state" data-tone={tone}>
        <ToolIcon name={toolStateIcon(tone)} size={14} />
        {text}
      </span>
      {open === undefined ? null : (
        <span className="agent-tool-chevron" aria-hidden="true">
          <ExpandIcon open={open} />
        </span>
      )}
      {entry.display.summary ? (
        <span className="agent-tool-brief">{entry.display.summary}</span>
      ) : null}
      {entry.remote_decision ? (
        <span className="agent-tool-brief">{remoteDecisionText(entry.remote_decision)}</span>
      ) : null}
    </span>
  );
}

/** A call with bounded diagnostics: the row stays a real disclosure, and its
 *  full detail opens in the expanded body. */
function ExpandableToolActivity({ entry }: { entry: AgentToolActivityEntry }) {
  const { onToggle, open } = useDisclosureOpen();
  const { outcome } = stateOf(entry);
  return (
    <details
      className="agent-tool-activity"
      data-outcome={outcome ?? entry.summary.status}
      onToggle={onToggle}
    >
      <summary className="agent-tool-row" data-running={callUnderWay(entry) || undefined}>
        <LeadingSlot kind={toolTypeIcon(entry.summary.display_name)} />
        <ToolActivityBody entry={entry} open={open} />
      </summary>
      <div className="agent-tool-facts">
        {/* Drawn only once the row is opened. */}
        {open && entry.display.image ? (
          <BrowserScreenshot
            artifactId={entry.display.image}
            name={screenshotName(entry.summary.display_name)}
          />
        ) : null}
        {entry.display.details.length ? (
          <ScrollBox label={`${entry.display.title}的详情`}>
            <ul>
              {entry.display.details.map((line, index) => (
                <li key={index}>
                  <DetailLine
                    call={entry.summary.tool_call_id}
                    line={line}
                    tool={entry.summary.display_name}
                  />
                </li>
              ))}
            </ul>
          </ScrollBox>
        ) : null}
      </div>
    </details>
  );
}

/** A call the model wrote that Core has no record of: being prepared, or never run. */
function UnrecordedCall({ ended }: { ended: boolean }) {
  return (
    <div className="agent-tool-activity" data-outcome={ended ? 'not_executed' : 'queued'}>
      <div className="agent-tool-row" data-running={ended ? undefined : true}>
        <LeadingSlot kind={toolTypeIcon('')} />
        <span className="agent-tool-main">
          <span className="agent-tool-title">工具调用</span>
          <span className="agent-tool-state" data-tone={ended ? 'unknown' : 'running'}>
            <ToolIcon name={toolStateIcon(ended ? 'unknown' : 'running')} size={14} />
            {ended ? '未执行' : '准备中'}
          </span>
        </span>
      </div>
    </div>
  );
}

/** A call with nothing to expand keeps the same row geometry, unclickable. */
function ToolActivityLine({ entry }: { entry: AgentToolActivityEntry }) {
  const { outcome } = stateOf(entry);
  return (
    <div className="agent-tool-activity" data-outcome={outcome ?? entry.summary.status}>
      <div className="agent-tool-row" data-running={callUnderWay(entry) || undefined}>
        <LeadingSlot kind={toolTypeIcon(entry.summary.display_name)} />
        <ToolActivityBody entry={entry} />
      </div>
    </div>
  );
}
