// App-wide preferences. Grouped under short headings so this stays easy to
// grow: a new setting is another <div className="setting-row"> inside an
// existing .settings-group, or a new group entirely — see styles.css's
// .settings-view rules for the shell this reuses from the activity log.

import type { Theme } from "../ui/useTheme";
import type { DevModeControls } from "../ui/useDevMode";

interface SettingsPanelProps {
  theme: Theme;
  onToggleTheme: () => void;
  /// WP-16. This row is the *only* thing about dev mode the app shows while the
  /// mode is off — the control that turns it on, and nothing else.
  devMode: DevModeControls;
}

export function SettingsPanel({ theme, onToggleTheme, devMode }: SettingsPanelProps) {
  const isLight = theme === "light";
  const isDev = devMode.enabled === true;
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

      {/* WP-16. Its own group rather than a row under Appearance: dev mode is a
          posture the app runs in, not a preference about how it looks, and the
          packages that live inside it (the evaluator, the fence) will file their
          own rows here. */}
      <div className="settings-group">
        <h4 className="settings-group__title">Development</h4>
        <div className="setting-row">
          <div className="setting-row__text">
            <span className="setting-row__label">Dev mode</span>
            <span className="setting-row__desc">
              The posture for evaluating the fleet rather than working with it. A coral band across
              the top says so for as long as it is on, and the setting survives a restart.
            </span>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={isDev}
            aria-label="Dev mode"
            // Until the backend has answered there is nothing honest to toggle:
            // a switch that starts at "off" and jumps would misreport the mode
            // for exactly as long as anyone is looking at it.
            disabled={devMode.enabled === null}
            className={`switch ${isDev ? "switch--on" : ""}`}
            onClick={devMode.toggle}
          >
            <span className="switch__thumb" />
          </button>
        </div>
        {devMode.error && <p className="setting-row__error">{devMode.error}</p>}
      </div>
    </div>
  );
}
