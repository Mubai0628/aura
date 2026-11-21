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
import { shownDetails } from './tool-activity-details';
import { useUiSettings } from '../ui-settings-store';
import { useContext } from 'react';
import { ToolRecordReads } from './useToolCallDirectory';

export function ToolActivity({
  entry,
  callId,
  ended = false,
}: {
  entry: AgentToolActivityEntry | undefined;
  callId?: string;
  /** The turn is over: a call with no record by then never ran. */
  ended?: boolean;
}) {
  const debug = useUiSettings().settings.show_debug_information;
  // A call Core has not recorded yet (its batch not admitted) is shown with its state.
  if (!entry) return <UnrecordedCall callId={callId} ended={ended} />;
  if (entry.display.internal) return null;
  const details = shownDetails(entry.display.details, debug);
  return (
    <>
      {details.length > 0 || entry.display.image ? (
        <ExpandableToolActivity entry={entry} details={details} />
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
function ExpandableToolActivity({
  entry,
  details,
}: {
  entry: AgentToolActivityEntry;
  details: readonly string[];
}) {
  const { onToggle, open } = useDisclosureOpen();
  const { outcome } = stateOf(entry);
  const found = entry.summary.outcome?.type === 'succeeded';
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
        {details.length ? (
          <ScrollBox label={`${entry.display.title}的详情`}>
            <ul>
              {details.map((line, index) => (
                <li key={index}>
                  <DetailLine
                    call={entry.summary.tool_call_id}
                    found={found}
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

/** A call the model wrote that Core has no record of: being prepared, never run, or of unknown
 *  state when its record could not be read. */
function UnrecordedCall({ callId, ended }: { callId: string | undefined; ended: boolean }) {
  const reads = useContext(ToolRecordReads);
  const unreadable = callId !== undefined && reads.unreadable.has(callId);
  const absent = ended && callId !== undefined && reads.missing.has(callId);
  const [state, brief] = unreadable
    ? ['状态未知', '读不到这个调用的记录，状态未知；稍后会自动再试。']
    : absent
      ? ['未执行', '这个调用在开始前随这一步结束了，没有执行，什么都没改。']
      : [ended ? '正在读取记录' : '准备中', undefined];
  const tone = absent || unreadable ? 'unknown' : 'running';
  return (
    <div
      className="agent-tool-activity"
      data-outcome={unreadable ? 'unknown' : absent ? 'not_executed' : 'queued'}
    >
      <div className="agent-tool-row" data-running={tone === 'running' ? true : undefined}>
        <LeadingSlot kind={toolTypeIcon('')} />
        <span className="agent-tool-main">
          <span className="agent-tool-title">工具调用</span>
          <span className="agent-tool-state" data-tone={tone}>
            <ToolIcon name={toolStateIcon(tone)} size={14} />
            {state}
          </span>
          {brief ? <span className="agent-tool-brief">{brief}</span> : null}
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
