import { useThemePlugins, type ThemeOption } from './theme-plugins';

/** Aura's own theme or a ready plugin's. A chosen theme that cannot be used now stays shown. */
export function ThemeChoice({
  chosen,
  disabled,
  onChoose,
}: {
  chosen: string | null | undefined;
  disabled: boolean;
  onChoose: (name: string | null) => void;
}) {
  const themes = useThemePlugins();
  const options: ThemeOption[] = [{ name: null, title: 'Aura 默认' }, ...(themes ?? [])];
  if (chosen && !options.some((option) => option.name === chosen))
    options.push({ name: chosen, title: themes ? `${chosen}（现在不能用）` : chosen });
  return (
    <div className="environment-choice">
      <span id="environment-theme">主题</span>
      <div role="radiogroup" aria-labelledby="environment-theme">
        {options.map((option) => (
          <label key={option.name ?? ''}>
            <input
              type="radio"
              name="plugin-theme"
              checked={(chosen ?? null) === option.name}
              disabled={disabled}
              onChange={() => onChoose(option.name)}
            />
            {option.title}
          </label>
        ))}
      </div>
    </div>
  );
}
