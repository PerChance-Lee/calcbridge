import { openUrl } from "@tauri-apps/plugin-opener";
import { useApp, type Settings } from "../store";

const REPO = "https://github.com/PerChance-Lee/calcbridge";
const TI_OS = "https://education.ti.com/en/products/calculators/graphing-calculators/ti-nspire-cx-ii";

function Toggle({ k, label, desc }: { k: keyof Settings; label: string; desc: string }) {
  const { settings, updateSettings } = useApp();
  return (
    <label className="row check" style={{ justifyContent: "space-between" }}>
      <span>
        <div>{label}</div>
        <div className="desc">{desc}</div>
      </span>
      <input type="checkbox" checked={Boolean(settings[k])} onChange={(e) => updateSettings({ [k]: e.target.checked } as Partial<Settings>)} />
    </label>
  );
}

export function SettingsView() {
  const { settings, updateSettings } = useApp();
  const link = (url: string, text: string) => (
    <a href={url} onClick={(e) => (e.preventDefault(), openUrl(url))}>
      {text}
    </a>
  );
  return (
    <div className="view">
      <div className="view-head">
        <h1>Settings &amp; about</h1>
      </div>
      <div className="settings">
        <div className="card">
          <h2>Appearance</h2>
          <div className="row">
            <span>Theme</span>
            <div className="seg">
              {(["system", "light", "dark"] as const).map((t) => (
                <button key={t} className={settings.theme === t ? "on" : ""} onClick={() => updateSettings({ theme: t })}>
                  {t[0].toUpperCase() + t.slice(1)}
                </button>
              ))}
            </div>
          </div>
        </div>
        <div className="card">
          <h2>Connection</h2>
          <Toggle k="autoConnect" label="Connect automatically" desc="Connect as soon as a calculator is plugged in." />
        </div>
        <div className="card">
          <h2>Files</h2>
          <Toggle k="confirmDelete" label="Ask before deleting" desc="The handheld has no recycle bin." />
          <Toggle k="showHidden" label="Show hidden files on this computer" desc="Dot files and files marked hidden or system." />
        </div>
        <div className="card">
          <div className="about">
            <img src="/icon.png" alt="" />
            <div>
              <h2>CalcBridge 2.0</h2>
              <div className="desc" style={{ color: "var(--text-2)" }}>
                A free, open-source link for the TI-Nspire CX II: files, live screen, remote keypad, backups, OS installs and documents with real 2D
                math.
              </div>
            </div>
          </div>
          <div style={{ color: "var(--text-2)", fontSize: 12.5, display: "flex", flexDirection: "column", gap: 6, userSelect: "text" }}>
            <span>
              {link(REPO, "Project page and source code")} · {link(`${REPO}/issues`, "Report a problem")} · {link(TI_OS, "TI OS downloads")}
            </span>
            <span>
              Licensed under the GNU GPL v3 or later. Protocol knowledge from libnspire, tilibs, TnsTools and hackspire.org. Built with Tauri, nusb,
              React and KaTeX.
            </span>
            <span style={{ color: "var(--text-3)" }}>
              Not affiliated with or endorsed by Texas Instruments. TI-Nspire is a trademark of Texas Instruments.
            </span>
          </div>
        </div>
      </div>
    </div>
  );
}
