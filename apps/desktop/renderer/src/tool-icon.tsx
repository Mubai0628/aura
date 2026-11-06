import { PixelIcon } from './pixel-icons/PixelIcon';
import { TOOL_ART, type ToolIconName } from './pixel-icons/tool-art';

/** One fixed 16px slot per icon role. Expansion, Tool type and state are
 *  different responsibilities and never share a character stand-in. */
export function ToolIcon({
  name,
  size = 16,
  className,
}: {
  name: ToolIconName;
  size?: number;
  className?: string;
}) {
  return (
    <PixelIcon
      art={TOOL_ART[name]}
      className={className === undefined ? 'tool-icon' : `tool-icon ${className}`}
      data-icon={name}
      size={size}
    />
  );
}

/** Expand/collapse affordance. The rotation is a real state change, not a loop. */
export function ExpandIcon({ open }: { open: boolean }) {
  return <ToolIcon className="tool-icon-expand" name={open ? 'chevron-down' : 'chevron-right'} />;
}
