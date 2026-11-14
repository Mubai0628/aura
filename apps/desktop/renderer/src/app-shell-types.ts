export type AppScreen = 'settings' | 'workspace';

export type SettingsSection =
  | 'models'
  | 'security'
  | 'tools'
  | 'environment'
  | 'browser'
  | 'computer'
  | 'memory'
  | 'commands'
  | 'agents'
  | 'plugins'
  | 'hooks'
  | 'mcp'
  | 'skills'
  | 'archive'
  | 'notifications'
  | 'storage'
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
  /** Core could not open it at its folder: the folder is gone or is another folder now. */
  offline?: boolean;
}

export function requiredFirst<T>(values: readonly T[], source: string): T {
  const value = values[0];
  if (value === undefined) {
    throw new Error(`${source} must contain at least one item.`);
  }
  return value;
}
