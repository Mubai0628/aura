import { useAppUpdates } from './useAppUpdates';
import './update-dot.css';

function LiveUpdateDot() {
  const { state } = useAppUpdates();
  return state?.available && state.latest ? (
    <span aria-hidden="true" className="update-dot" />
  ) : null;
}

/**
 * A red dot while a newer version of Aura waits to be installed. It only draws attention: the new
 * version itself is announced by a notification. A page without Aura's desktop bridge has none.
 */
export function UpdateDot() {
  return typeof window.aura?.v1?.appUpdate === 'function' ? <LiveUpdateDot /> : null;
}
