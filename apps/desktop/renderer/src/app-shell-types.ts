export type AppScreen = 'settings' | 'workspace';

export type SettingsSection =
  | 'models'
  | 'security'
  | 'tools'
  | 'environment'
  | 'browser'
  | 'memory'
  | 'commands'
  | 'plugins'
  | 'hooks'
  | 'mcp'
  | 'skills'
  | 'archive'
  | 'about';

export interface CoreStatusPresentation {
  label: string;
  tone: 'danger' | 'muted' | 'success' | 'warning';
  /** More about the status, on hover. */
  detail?: string;
}

/** The kind of the right sidebar's open tab, or none. */
export type RightPanel = import('./right-workspace/tool-tabs').ToolKind | 'empty';

export interface SessionNavigationItem {
  id: string;
  state: string;
  title: string;
}

export interface WorkspaceNavigationItem {
  id: string;
  name: string;
  root: string;
  sessions: SessionNavigationItem[];
}

export function requiredFirst<T>(values: readonly T[], source: string): T {
  const value = values[0];
  if (value === undefined) {
    throw new Error(`${source} must contain at least one item.`);
  }
  return value;
}
