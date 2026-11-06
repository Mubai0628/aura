import { AppIcon } from '../app-icons';
import { useProgramIcon } from './program-icons';

/** A program's own icon, or the plain program icon until it is read and when it has none. */
export function ProgramIcon({
  appId,
  size,
  className,
}: {
  appId: string | undefined;
  size: number;
  className?: string;
}) {
  const icon = useProgramIcon(appId);
  return icon ? (
    <img
      className={`program-icon${className ? ` ${className}` : ''}`}
      src={icon}
      alt=""
      aria-hidden="true"
      width={size}
      height={size}
      draggable={false}
    />
  ) : (
    <AppIcon name="desktop" size={size} className={className} />
  );
}
