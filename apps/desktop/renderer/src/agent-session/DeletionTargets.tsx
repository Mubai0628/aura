import { deletingLines } from './deleting-lines';

/**
 * Under the warning that a
 * deletion is permanent, what it deletes — the file `fs.delete` names, or the lines of a Shell
 * script that delete.
 */
export function DeletionTargets({
  actionKind,
  action,
}: {
  actionKind: string;
  action: Record<string, unknown>;
}) {
  const file = actionKind === 'fs.delete';
  const items = file
    ? typeof action.path === 'string'
      ? [{ key: 'path', label: undefined, text: action.path }]
      : []
    : deletingLines(typeof action.script === 'string' ? action.script : '').map((line) => ({
        key: String(line.line),
        label: `第 ${line.line} 行`,
        text: line.text,
      }));
  if (!items.length) return null;
  return (
    <ul aria-label={file ? '删除的文件' : '删除命令'} className="agent-approval-deletions">
      {items.map((item) => (
        <li key={item.key}>
          {item.label ? <span>{item.label}</span> : null}
          <code>{item.text}</code>
        </li>
      ))}
    </ul>
  );
}
