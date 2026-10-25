import * as Switch from '@radix-ui/react-switch';
import './controls.css';

export function ToggleSwitch({
  checked,
  disabled,
  label,
  onCheckedChange,
}: {
  checked: boolean;
  disabled?: boolean;
  label: string;
  onCheckedChange: (checked: boolean) => void;
}) {
  return (
    <Switch.Root
      aria-label={label}
      checked={checked}
      disabled={disabled}
      onCheckedChange={onCheckedChange}
      className="aura-switch"
    >
      <Switch.Thumb className="aura-switch-thumb" />
    </Switch.Root>
  );
}
