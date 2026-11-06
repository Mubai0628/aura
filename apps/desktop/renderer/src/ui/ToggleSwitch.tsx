import * as Switch from '@radix-ui/react-switch';
import './controls.css';

/** Radix sets `data-state` before the props it is given, so this removes it from both parts. */
const NO_STATE = { 'data-state': undefined };

export function ToggleSwitch({
  checked,
  disabled,
  label,
  onCheckedChange,
}: {
  /** `undefined` while the value is still being read: the switch shows no thumb and is disabled. */
  checked: boolean | undefined;
  disabled?: boolean;
  label: string;
  onCheckedChange: (checked: boolean) => void;
}) {
  const unknown = checked === undefined;
  return (
    <Switch.Root
      aria-busy={unknown || undefined}
      aria-label={label}
      checked={checked ?? false}
      disabled={disabled || unknown}
      onCheckedChange={onCheckedChange}
      className="aura-switch"
      {...(unknown ? NO_STATE : {})}
    >
      <Switch.Thumb className="aura-switch-thumb" {...(unknown ? NO_STATE : {})} />
    </Switch.Root>
  );
}
