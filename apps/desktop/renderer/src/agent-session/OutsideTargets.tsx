import { outsideLines } from './outside-lines';

/** Under the warning that a command writes outside the workspace, the lines that name where. */
export function OutsideTargets({ text }: { text: string }) {
  const items = outsideLines(text);
  if (!items.length) return null;
  return (
    <ul aria-label="写到工作区以外的命令" className="agent-approval-deletions">
      {items.map((item) => (
        <li key={item.line}>
          <span>第 {item.line} 行</span>
          <code>{item.text}</code>
        </li>
      ))}
    </ul>
  );
}
