import { useState } from 'react';
import { userMessage } from './user-message';
import type {
  AgentPromptInspection,
  PromptInspectorActivitySourceView,
} from '@aura/contracts/generated';
import type { WorkspaceNavigationItem } from '../app-shell-types';
import { PromptInspectorShell } from '../PromptInspectorShell';
import { PixelBar, PixelCard } from '../inspector/PixelBar';
import { FORMAT_LABELS, SECTION_LABELS, amount, share } from '../inspector/inspector-labels';
import { pixelCells } from '../inspector/pixel-cells';

/** How many of the step's tools are drawn; the rest are counted. */
const TOP_TOOLS = 6;

/** The workspace and conversation that run another task, by the names the sidebar shows. */
function sourceName(
  workspaces: readonly WorkspaceNavigationItem[],
  source: PromptInspectorActivitySourceView,
): string {
  const workspace = workspaces.find((item) => item.id === source.workspace_id);
  const session = workspace?.sessions.find((item) => item.id === source.session_id);
  return `${workspace?.name ?? '未知工作区'} › ${session ? session.title || '新会话' : '未知会话'}`;
}

export function Inspector({
  view,
  error,
  workspaces = [],
}: {
  view: AgentPromptInspection | undefined;
  error: string | undefined;
  /** Names the conversations behind the other tasks; the model never sees them. */
  workspaces?: readonly WorkspaceNavigationItem[] | undefined;
}) {
  // The snapshot is laid out only once its details are opened.
  const [detailsOpen, setDetailsOpen] = useState(false);
  if (!view)
    return (
      <div>
        {error ? (
          <p role="alert">{userMessage(error)}</p>
        ) : (
          <p role="status">在消息中选择“检查此步骤”，查看它实际使用的配置。</p>
        )}
      </div>
    );
  const projection = view.projection;
  const input = projection.cache_usage.actual_input_tokens;
  const cached = projection.cache_usage.actual_cached_input_tokens;
  const hit = input != null && cached != null && input > 0 ? { input, cached } : undefined;
  // The conversation's finished requests that
  // reported their cache use, together.
  const total =
    view.session_cache && view.session_cache.input_tokens > 0 ? view.session_cache : undefined;
  const thisStep = hit ? share(hit.cached, hit.input) : input == null ? '未完成' : '未报告';
  // Without the conversation's figures (an older Core), the step's own.
  const shownCache = total ? { cached: total.cached_input_tokens, input: total.input_tokens } : hit;
  const largestSection = Math.max(1, ...projection.sections.map((s) => s.estimated_tokens));
  const tools = [...projection.tools].sort((a, b) => b.estimated_tokens - a.estimated_tokens);
  const shownTools = tools.slice(0, TOP_TOOLS);
  const otherTools = tools.slice(TOP_TOOLS);
  const largestTool = Math.max(1, ...shownTools.map((tool) => tool.estimated_tokens));
  return (
    <div className="agent-inspector pixel-stack">
      <PromptInspectorShell
        projection={projection}
        chips={
          <span className="pixel-chips">
            <span className="pixel-chip">{view.execution.model_name}</span>
            {/* Which connection carried this step (the provider's default, or a backup). */}
            <span className="pixel-chip is-quiet">{view.execution.endpoint_display_name}</span>
            <span className="pixel-chip is-quiet">
              {FORMAT_LABELS[view.execution.api_format] ?? view.execution.api_format}
            </span>
          </span>
        }
      />
      <PixelCard
        title="缓存"
        figure={shownCache ? share(shownCache.cached, shownCache.input) : '未报告'}
      >
        {shownCache ? (
          <>
            <PixelBar
              cells={pixelCells(
                [{ key: 'cached', value: shownCache.cached }],
                shownCache.input,
                24,
              )}
              label={`${total ? '会话缓存' : '缓存'} ${amount(shownCache.cached)} / ${amount(shownCache.input)}`}
            />
            <p className="pixel-caption">
              {total
                ? `会话 ${amount(total.cached_input_tokens)} / ${amount(total.input_tokens)} · ${total.reported_steps} 次请求`
                : `${amount(shownCache.cached)} / ${amount(shownCache.input)}`}
            </p>
          </>
        ) : null}
        {total ? <p className="pixel-caption">本步 {thisStep}</p> : null}
      </PixelCard>
      {projection.sections.length ? (
        <PixelCard title="模块">
          <ol className="pixel-rows">
            {projection.sections.map((section) => (
              <li key={section.physical_ordinal}>
                <span className="pixel-row-name">
                  {SECTION_LABELS[section.kind] ?? section.kind}
                  {section.changed_from_previous_step ? (
                    <span aria-label="与上一步不同" className="pixel-changed" role="img" />
                  ) : null}
                </span>
                <PixelBar
                  cells={pixelCells(
                    [{ key: 'part', value: section.estimated_tokens }],
                    largestSection,
                    16,
                  )}
                  label={`${SECTION_LABELS[section.kind] ?? section.kind} ${amount(
                    section.estimated_tokens,
                  )}`}
                />
                <b>{amount(section.estimated_tokens)}</b>
              </li>
            ))}
          </ol>
        </PixelCard>
      ) : null}
      {view.activity_sources?.length ? (
        <PixelCard title="其它任务" figure={view.activity_sources.length}>
          <ol className="pixel-rows is-sources">
            {view.activity_sources.map((source) => (
              <li key={source.another_task}>
                <span className="pixel-row-name">任务 {source.another_task}</span>
                <b>{sourceName(workspaces, source)}</b>
              </li>
            ))}
          </ol>
        </PixelCard>
      ) : null}
      {tools.length ? (
        <PixelCard title="工具" figure={tools.length}>
          <ol className="pixel-rows is-tools">
            {shownTools.map((tool) => (
              <li key={tool.tool_id}>
                <span className="pixel-row-name is-code">{tool.canonical_name}</span>
                <PixelBar
                  cells={pixelCells(
                    [{ key: 'tool', value: tool.estimated_tokens }],
                    largestTool,
                    16,
                  )}
                  label={`${tool.canonical_name} ${amount(tool.estimated_tokens)}`}
                />
                <b>{amount(tool.estimated_tokens)}</b>
              </li>
            ))}
          </ol>
          {otherTools.length ? (
            <p className="pixel-caption">
              其余 {otherTools.length} 个 ·{' '}
              {amount(otherTools.reduce((sum, tool) => sum + tool.estimated_tokens, 0))}
            </p>
          ) : null}
        </PixelCard>
      ) : null}
      <details
        className="pixel-details"
        onToggle={(event) => setDetailsOpen(event.currentTarget.open)}
      >
        <summary>快照元数据</summary>
        {detailsOpen ? (
          <>
            <dl>
              <dt>ModelStep</dt>
              <dd>{view.execution.model_step_id}</dd>
              <dt>Context</dt>
              <dd>{projection.context_snapshot_id}</dd>
              <dt>Prompt Bundle</dt>
              <dd>{view.execution.prompt_bundle_snapshot_id}</dd>
              <dt>模型配置修订</dt>
              <dd>{view.execution.model_config_revision}</dd>
            </dl>
          </>
        ) : null}
      </details>
      {error ? <p role="alert">{userMessage(error)}</p> : null}
    </div>
  );
}
