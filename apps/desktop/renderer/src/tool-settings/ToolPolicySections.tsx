import { AppIcon } from '../app-icons';
import { ExecutionStrategy } from './ExecutionStrategy';
import type { ConfigurationOwner } from './useAgentConfiguration';
import { WebService } from './WebService';
import './execution-strategy.css';
import { projectionNotRead } from './failure-words';

export function ToolPolicySections({
  configuration,
  strategyFailure,
  strategyReady,
}: {
  configuration: ConfigurationOwner;
  /** The code the tool settings subscription last failed with, while it is not read. */
  strategyFailure?: string | undefined;
  strategyReady: boolean;
}) {
  return (
    <>
      <section className="settings-surface tool-policy-surface">
        <header className="tool-section-heading">
          <div>
            <h3>执行策略</h3>
          </div>
        </header>
        {strategyReady ? (
          <ExecutionStrategy configuration={configuration} />
        ) : (
          <div className="tool-projection-placeholder" role="status">
            <AppIcon name="refresh" />
            <span>
              {strategyFailure === undefined
                ? '正在读取工具设置…'
                : projectionNotRead('工具设置', strategyFailure)}
            </span>
          </div>
        )}
      </section>
      <section className="settings-surface tool-policy-surface" aria-label="网页搜索与抓取">
        <header className="tool-section-heading">
          <div>
            <h3>网页搜索与抓取</h3>
          </div>
        </header>
        <WebService configuration={configuration} />
      </section>
    </>
  );
}
