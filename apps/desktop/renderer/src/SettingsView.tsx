import { ConfigRecoveryPanel } from './settings-recovery/ConfigRecoveryPanel';
import { ProjectionCompatibilityNotice } from './ProjectionCompatibilityNotice';
import { Fragment, useState } from 'react';

import { AppIcon, type IconName } from './app-icons';
import type { CoreStatusPresentation, SettingsSection } from './app-shell-types';
import { ModelSettings } from './ModelSettings';
import { SecuritySettings } from './SecuritySettings';
import { ToolSettings } from './ToolSettings';
import { EnvironmentSettings } from './environment-settings/EnvironmentSettings';
import { BrowserSettings } from './browser-settings/BrowserSettings';
import { ArchiveSettings } from './session-lifecycle/ArchiveSettings';
import { MemorySettings } from './memory-settings/MemorySettings';
import { CommandSettings } from './command-settings/CommandSettings';
import { HookSettings } from './hook-settings/HookSettings';
import { McpSettings } from './mcp-settings/McpSettings';
import { PluginSettings } from './plugin-settings/PluginSettings';
import { SkillSettings } from './skill-settings/SkillSettings';
import { AboutUpdates } from './app-updates/AboutUpdates';
import { NotificationSettings } from './notification-settings/NotificationSettings';
import './settings-navigation.css';
import type { WorkspaceId } from '@aura/contracts/generated';
import { useSecurityControl } from './security-control-store';
import type { NotificationInput } from './notification-center-store';
import { UpdateDot } from './app-updates/UpdateDot';

type SecurityControl = ReturnType<typeof useSecurityControl>;

interface SettingsViewProps {
  coreReady: boolean;
  coreStatus: CoreStatusPresentation;
  onChangeSection: (section: SettingsSection) => void;
  onClose: () => void;
  onNotify: (notification: NotificationInput) => void;
  /** Withdraws the notices a state raised once it is fine again (key prefix). */
  onWithdrawNotice: (prefix: string) => void;
  section: SettingsSection;
  security: SecurityControl;
  workspaceId?: WorkspaceId | undefined;
}

// Group titles are labels, not pages. Keywords match a page's controls as well as its name.
const SETTINGS_NAVIGATION: readonly {
  group: string;
  icon: IconName;
  label: string;
  value: SettingsSection;
  keywords?: readonly string[];
}[] = [
  { group: '基础', icon: 'model', label: '模型', value: 'models' },
  { group: '基础', icon: 'shield', label: '权限与沙箱', value: 'security' },
  { group: '基础', icon: 'wrench', label: '工具', value: 'tools' },
  {
    group: '基础',
    icon: 'terminal',
    label: '环境',
    value: 'environment',
    keywords: ['git', 'powershell', 'cmd', '终端'],
  },
  {
    group: '基础',
    icon: 'browser',
    label: '网络与浏览器',
    value: 'browser',
    keywords: ['浏览器', '网站', '阻止', '调试'],
  },
  { group: '智能能力', icon: 'bookmark', label: '记忆', value: 'memory' },
  { group: '智能能力', icon: 'slash', label: '命令', value: 'commands' },
  { group: '扩展', icon: 'puzzle', label: '插件', value: 'plugins' },
  { group: '扩展', icon: 'book', label: 'Skill', value: 'skills' },
  { group: '扩展', icon: 'plug', label: 'MCP', value: 'mcp' },
  { group: '扩展', icon: 'activity', label: 'Hook', value: 'hooks' },
  {
    group: '系统',
    icon: 'archive',
    label: '归档',
    value: 'archive',
    keywords: ['删除', '恢复', '会话', '工作区'],
  },
  {
    group: '系统',
    icon: 'bell',
    label: '通知',
    value: 'notifications',
    keywords: ['提醒', '审批', '完成'],
  },
  {
    group: '系统',
    icon: 'refresh',
    label: '关于与更新',
    value: 'about',
    keywords: ['版本', '更新', '下载', '回退', '关于'],
  },
];
const SETTINGS_GROUPS = ['基础', '智能能力', '扩展', '系统'] as const;

export function SettingsView({
  coreReady,
  coreStatus,
  onChangeSection,
  onClose,
  onNotify,
  onWithdrawNotice,
  section,
  security,
  workspaceId,
}: SettingsViewProps) {
  const [query, setQuery] = useState('');
  const normalizedQuery = query.trim().toLocaleLowerCase('zh-CN');
  const visibleNavigation = SETTINGS_NAVIGATION.filter((item) =>
    [item.label, ...(item.keywords ?? [])].some((word) =>
      word.toLocaleLowerCase('zh-CN').includes(normalizedQuery),
    ),
  );

  return (
    <main className="settings-workspace">
      <header className="settings-header">
        <div className="settings-header-leading">
          <button className="settings-back-button" onClick={onClose} type="button">
            <AppIcon name="arrow-left" size={16} />
            返回会话
          </button>
          <div>
            <h1>设置</h1>
          </div>
        </div>
        <div className="settings-header-actions">
          <label className="settings-search field-frame">
            <AppIcon name="search" size={15} />
            <span className="sr-only">搜索设置</span>
            <input
              onChange={(event) => setQuery(event.target.value)}
              placeholder="搜索设置"
              type="search"
              value={query}
            />
            <kbd>Ctrl+F</kbd>
          </label>
          <span className={`settings-core-status state-${coreStatus.tone}`}>
            <span aria-hidden="true" />
            {coreStatus.label}
          </span>
        </div>
      </header>

      <div className="settings-layout">
        <nav aria-label="设置分类" className="settings-navigation">
          {SETTINGS_GROUPS.map((group) => {
            const items = visibleNavigation.filter((item) => item.group === group);
            return items.length ? (
              <Fragment key={group}>
                <p className="settings-navigation-group">{group}</p>
                {items.map((item) => (
                  <button
                    aria-current={section === item.value ? 'page' : undefined}
                    className={section === item.value ? 'is-active' : undefined}
                    key={item.value}
                    onClick={() => onChangeSection(item.value)}
                    type="button"
                  >
                    <AppIcon name={item.icon} />
                    <span>{item.label}</span>
                    {item.value === 'about' ? <UpdateDot /> : null}
                  </button>
                ))}
              </Fragment>
            ) : null;
          })}
          {visibleNavigation.length === 0 ? (
            <p className="settings-no-results">没有匹配的设置。</p>
          ) : null}
        </nav>

        <div className="settings-content">
          {section !== 'about' && section !== 'notifications' ? (
            <>
              <ProjectionCompatibilityNotice surface={section} />
              <ConfigRecoveryPanel coreReady={coreReady} />
            </>
          ) : null}
          {section === 'about' ? <AboutUpdates /> : null}
          {section === 'models' ? (
            <ModelSettings onNotify={onNotify} onWithdrawNotice={onWithdrawNotice} />
          ) : null}
          {section === 'tools' ? <ToolSettings /> : null}
          {section === 'environment' ? <EnvironmentSettings /> : null}
          {section === 'browser' ? <BrowserSettings coreReady={coreReady} /> : null}
          {section === 'archive' ? <ArchiveSettings /> : null}
          {section === 'notifications' ? <NotificationSettings /> : null}
          {section === 'memory' ? (
            <MemorySettings
              key={workspaceId ?? 'global'}
              coreReady={coreReady}
              workspaceId={workspaceId}
            />
          ) : null}
          {section === 'commands' ? (
            <CommandSettings
              key={workspaceId ?? 'global'}
              coreReady={coreReady}
              workspaceId={workspaceId}
            />
          ) : null}
          {section === 'plugins' ? (
            <PluginSettings coreReady={coreReady} onOpenSection={onChangeSection} />
          ) : null}
          {section === 'hooks' ? <HookSettings coreReady={coreReady} /> : null}
          {section === 'mcp' ? <McpSettings coreReady={coreReady} /> : null}
          {section === 'skills' ? (
            <SkillSettings
              key={workspaceId ?? 'global'}
              coreReady={coreReady}
              workspaceId={workspaceId}
            />
          ) : null}
          {section === 'security' ? (
            <SecuritySettings onNotify={onNotify} security={security} />
          ) : null}
        </div>
      </div>
    </main>
  );
}
