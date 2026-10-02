// App-wide preferences. Grouped under short headings so this stays easy to
// grow: a new setting is another <div className="setting-row"> inside an
// existing .settings-group, or a new group entirely — see styles.css's
// .settings-view rules for the shell this reuses from the activity log.

import type { Theme } from "../ui/useTheme";

interface SettingsPanelProps {
  theme: Theme;
  onToggleTheme: () => void;
}

export function SettingsPanel({ theme, onToggleTheme }: SettingsPanelProps) {
  const isLight = theme === "light";
  return (
    <div className="settings-view">
      <div className="settings-view__head">
        <h3>Settings</h3>
        <span className="label">preferences</span>
      </div>

      <div className="settings-group">
        <h4 className="settings-group__title">Appearance</h4>
        <div className="setting-row">
          <div className="setting-row__text">
            <span className="setting-row__label">Light mode</span>
            <span className="setting-row__desc">
              Switch the whole app, terminals included, to a light surface.
            </span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={isLight}
            aria-label="Light mode"
            className={`switch ${isLight ? "switch--on" : ""}`}
            onClick={onToggleTheme}
          >
            <span className="switch__thumb" />
          </button>
        </div>
      </div>
    </div>
  );
}
