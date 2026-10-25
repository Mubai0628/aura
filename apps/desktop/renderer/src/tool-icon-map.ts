import type { LucideIconName } from './tool-icon-paths';

/** Tool-type icon chosen from the stable canonical Tool name, never from a
 *  Chinese label or model prose. Unmapped Tools keep the neutral wrench. */
export function toolTypeIcon(canonicalName: string | undefined): LucideIconName {
  const name = canonicalName ?? '';
  const prefix = name.includes('.') ? name.slice(0, name.indexOf('.')) : name;
  const exact: Record<string, LucideIconName> = {
    'fs.list': 'folder-tree',
    'fs.glob': 'folder-tree',
    'fs.read': 'file-text',
    'fs.read_text': 'file-text',
    'fs.search': 'file-search',
    // Filename lookup is the same user-facing action as the historical
    // `fs.find_paths`, so it keeps that icon under its real canonical name.
    'fs.find': 'file-search',
    'fs.find_paths': 'file-search',
    'fs.write': 'file-pen-line',
    'fs.patch': 'file-pen-line',
    // Deleting into the Recycle Bin.
    'fs.delete': 'trash',
    'fs.hash': 'file-code',
    'fs.inspect': 'file-code',
    'shell.powershell': 'terminal',
    'shell.cmd': 'terminal',
    'shell.query': 'terminal',
    'shell.stop': 'terminal',
    'web.search': 'globe',
    'web.fetch': 'globe',
    'web.read': 'globe',
    'tools.search': 'package-search',
    'tools.describe': 'package-search',
    'tools.load': 'package-plus',
    'tools.unload': 'package-minus',
    'tools.list_loaded': 'package-search',
    'todo.update': 'list-checks',
    'plan.complete': 'list-checks',
    'goal.check': 'target',
    'goal.complete': 'target',
    'query.user_question': 'circle-question-mark',
    'task_lineage.read_sources': 'link',
    'task_lineage.review_completion': 'check',
    'normal.complete': 'check',
    'mode.disposition': 'activity',
    'context.read_events': 'file-code',
  };
  const mapped = exact[name];
  if (mapped !== undefined) return mapped;
  switch (prefix) {
    case 'fs':
      return 'file-text';
    case 'shell':
      return 'terminal';
    case 'web':
      return 'globe';
    case 'tools':
      return 'package-search';
    case 'task_lineage':
      return 'link';
    default:
      return 'wrench';
  }
}

export type ToolIconTone = 'success' | 'failed' | 'waiting' | 'running' | 'unknown';

/** Real state as an icon next to real state text; colour is never the only cue. */
export function toolStateIcon(tone: ToolIconTone): LucideIconName {
  switch (tone) {
    case 'success':
      return 'check';
    case 'failed':
      return 'circle-alert';
    case 'waiting':
      return 'pause';
    case 'running':
      return 'loader-circle';
    case 'unknown':
      return 'circle-question-mark';
  }
}
