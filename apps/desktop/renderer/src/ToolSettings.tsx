import { useMemo } from 'react';

import { AppIcon } from './app-icons';
import { useScopedToolProjection } from './tool-projection-store';
import { AgentToolList } from './tool-settings/AgentToolList';
import { ToolPolicySections } from './tool-settings/NetworkProxy';
import { useAgentConfiguration } from './tool-settings/useAgentConfiguration';
import { ToolPackagesPanel } from './tool-packages/ToolPackagesPanel';

/**
 * Tools and extensions. Tool packages have their own page section
 * (`ToolPackagesPanel`); the P07 Draft and package views were retired with
 * their routes.
 */
export function ToolSettings() {
  // One committed snapshot and one revision for both editors on this page, so any
  // save order works instead of the second save failing on a stale revision.
  const configuration = useAgentConfiguration();
  const settingsRoute = useMemo(
    () => ({ projection_kind: 'tool_settings', projection_scope: { type: 'global' } }) as const,
    [],
  );
  const settings = useScopedToolProjection(settingsRoute)?.payload;

  return (
    <section className="tool-settings settings-page">
      <div className="settings-content-heading">
        <div>
          <h2>工具与扩展</h2>
        </div>
        {settings === undefined ? null : (
          <span className="projection-revision-badge">策略修订 {settings.settings.revision}</span>
        )}
      </div>

      <AgentToolList configuration={configuration} />
      <ToolPackagesPanel />

      {settings?.configuration_issue === undefined ||
      settings.configuration_issue === null ? null : (
        <div className="tool-settings-issue" role="alert">
          <AppIcon name="warning" />
          <span>
            <strong>工具策略仍使用最后有效配置</strong>
            {settings.configuration_issue.message}
          </span>
        </div>
      )}

      <div className="tool-settings-grid">
        <ToolPolicySections configuration={configuration} strategyReady={settings !== undefined} />
      </div>
    </section>
  );
}
