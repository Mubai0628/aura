import { useRef } from 'react';

import { AppIcon } from './app-icons';
import { chooseSessionSpeed, speedLabel, useSessionSpeed } from './speed-mode';
import { useDismissablePopover } from './useDismissablePopover';
import './styles/model-picker.css';

/**
 * The composer's speed mode, the conversation's own choice for its model, beside the thinking
 * level: 标准, then each mode the model's settings allow. A model that allows none shows no
 * choice. In the composer's row its words give way first, with the thinking level's.
 */
export function SessionSpeedPicker({
  sessionId,
  modelProfileId,
  modes,
}: {
  sessionId: string;
  modelProfileId: string;
  modes: readonly string[];
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useDismissablePopover(ref);
  const chosen = useSessionSpeed(sessionId, modelProfileId);
  if (!modes.length) return null;
  const current = chosen !== undefined && modes.includes(chosen) ? chosen : null;
  const label = current === null ? '标准' : speedLabel(current);
  return (
    <div className="composer-menu-anchor speed-menu-anchor" ref={ref}>
      <button
        aria-expanded={open}
        aria-haspopup="menu"
        aria-label={`速度：${label}`}
        className="composer-select speed-select"
        data-adaptive
        data-adaptive-first
        data-on={current !== null}
        onClick={() => setOpen(!open)}
        title={`速度：${label}`}
        type="button"
      >
        <AppIcon name="bolt" size={14} />
        <span data-adaptive-label>{label}</span>
        <AppIcon className="adaptive-chevron" name="chevron-down" size={10} />
      </button>
      {open && (
        <div aria-label="速度" className="composer-popover speed-popover" role="menu">
          {[null, ...modes].map((mode) => {
            const checked = mode === current;
            return (
              <button
                aria-checked={checked}
                key={mode ?? 'standard'}
                onClick={() => {
                  chooseSessionSpeed(sessionId, modelProfileId, mode);
                  setOpen(false);
                }}
                role="menuitemradio"
                type="button"
              >
                <span>{mode === null ? '标准' : speedLabel(mode)}</span>
                {checked && <AppIcon name="check" size={14} />}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
