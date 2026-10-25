import { LUCIDE_ICON_PATHS, type LucideIconName } from './tool-icon-paths';

/** One fixed 16px slot per icon role. Expansion, Tool type and state are
 *  different responsibilities and never share a character stand-in. */
export function ToolIcon({
  name,
  size = 16,
  className,
}: {
  name: LucideIconName;
  size?: number;
  className?: string;
}) {
  return (
    <svg
      aria-hidden="true"
      className={className === undefined ? 'tool-icon' : `tool-icon ${className}`}
      data-icon={name}
      fill="none"
      height={size}
      stroke="currentColor"
      strokeLinecap="round"
      strokeLinejoin="round"
      strokeWidth={2}
      viewBox="0 0 24 24"
      width={size}
    >
      {LUCIDE_ICON_PATHS[name]}
    </svg>
  );
}

/** Expand/collapse affordance. The rotation is a real state change, not a loop. */
export function ExpandIcon({ open }: { open: boolean }) {
  return <ToolIcon className="tool-icon-expand" name={open ? 'chevron-down' : 'chevron-right'} />;
}
