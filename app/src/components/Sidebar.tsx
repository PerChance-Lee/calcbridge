import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, baseName, errorText, formatSize, joinLocal, type DeviceInfo } from "../api";
import { installOsFlow } from "../flows";
import {
  IconArchive, IconBook, IconChip, IconCreate, IconGear, IconKeypad, IconList, IconPlug, IconRefresh, IconRestore, IconScreen, IconFolder,
} from "../icons";
import { useApp, type Tab } from "../store";

const NAV: { tab: Tab; label: string; icon: typeof IconFolder; kbd: string }[] = [
  { tab: "files", label: "Files", icon: IconFolder, kbd: "Ctrl+1" },
  { tab: "screen", label: "Screen", icon: IconScreen, kbd: "Ctrl+2" },
  { tab: "keypad", label: "Remote keypad", icon: IconKeypad, kbd: "Ctrl+3" },
  { tab: "create", label: "Create", icon: IconCreate, kbd: "Ctrl+4" },
  { tab: "viewer", label: "Viewer", icon: IconBook, kbd: "Ctrl+5" },
  { tab: "log", label: "Log", icon: IconList, kbd: "Ctrl+6" },
];

export const TABS = NAV.map((n) => n.tab);

function stamp(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}-${p(d.getMinutes())}`;
}

export function Sidebar() {
  const { tab, setTab } = useApp();
  return (
    <aside className="sidebar">
      <div className="brand">
        <img src="/icon.png" alt="" />
        CalcBridge
        <small>2.0</small>
      </div>
      <DeviceCard />
      <nav className="nav">
        {NAV.map(({ tab: t, label, icon: Icon, kbd }) => (
          <button key={t} className={tab === t ? "active" : ""} onClick={() => setTab(t)} title={`${label} (${kbd})`}>
            <Icon />
            {label}
          </button>
        ))}
      </nav>
      <div className="sidebar-spacer" />
      <nav className="nav" style={{ paddingBottom: 10 }}>
        <button className={tab === "settings" ? "active" : ""} onClick={() => setTab("settings")}>
          <IconGear />
          Settings &amp; about
        </button>
      </nav>
    </aside>
  );
}

function Meter({ label, used, total }: { label: string; used: number; total: number }) {
  const pct = total > 0 ? Math.min(100, (used / total) * 100) : 0;
  return (
    <div className="meter" title={`${used.toLocaleString()} of ${total.toLocaleString()} bytes used`}>
      <div className="label">
        <span>{label}</span>
        <span>
          {formatSize(total - used)} free of {formatSize(total)}
        </span>
      </div>
      <div className="bar">
        <div style={{ width: `${pct}%`, background: pct > 90 ? "var(--danger)" : undefined }} />
      </div>
    </div>
  );
}

function battery(info: DeviceInfo): string {
  const b = info.battery === "unknown" ? "Not reported" : info.battery === "ok" ? "OK" : info.battery[0].toUpperCase() + info.battery.slice(1);
  return info.charging ? `${b}, charging` : b;
}

function DeviceCard() {
  const app = useApp();
  const [details, setDetails] = useState(false);
  const { status, info, simulated } = app;

  const connect = (sim: boolean) => api.connect(sim).catch((e) => app.toast("bad", "Couldn't connect", errorText(e)));

  async function backup() {
    const where = await open({ directory: true, title: "Choose a folder for the backup", defaultPath: app.settings.localFolder ?? undefined });
    if (typeof where !== "string") return;
    const places = await api.localPlaces();
    const name = `${(info?.name || "TI-Nspire").replace(/[<>:"/\\|?*]/g, "_")} backup ${stamp()}`;
    await api.backup(joinLocal(where, name, places.separator)).catch((e) => app.toast("bad", "Backup failed", errorText(e)));
  }

  async function restore() {
    const from = await open({ directory: true, title: "Choose a backup folder to restore", defaultPath: app.settings.localFolder ?? undefined });
    if (typeof from !== "string") return;
    const r = await app.ask({
      title: "Restore a backup",
      text: (
        <p>
          Send the TI-Nspire documents in <b>{baseName(from)}</b> to the handheld, keeping its folders. Anything that isn't a .tns
          document is left out.
        </p>
      ),
      input: { value: "/", label: "Into this handheld folder", hint: "/ is the top level (My Documents)." },
      checkbox: "Replace documents that are already there",
      choices: [
        { id: "cancel", label: "Cancel" },
        { id: "ok", label: "Restore", primary: true },
      ],
      cancel: "cancel",
    });
    if (r.choice !== "ok") return;
    const target = r.value.trim().startsWith("/") ? r.value.trim() : `/${r.value.trim()}`;
    await api.restore(from, target, r.checked).catch((e) => app.toast("bad", "Restore failed", errorText(e)));
  }

  let body;
  switch (status.state) {
    case "absent":
      body = (
        <>
          <div className="state">
            <span className="dot" /> No calculator
          </div>
          <div className="note">Plug in your TI-Nspire CX II with a USB cable. It connects by itself.</div>
          <div className="actions">
            <button className="btn small" onClick={() => connect(false)}>
              <IconPlug /> Connect
            </button>
            <button className="btn small ghost" onClick={() => connect(true)}>
              Try the simulator
            </button>
          </div>
        </>
      );
      break;
    case "present":
      body = (
        <>
          <div className="state">
            <span className="dot" /> Calculator found
          </div>
          <div className="actions">
            <button className="btn small primary" onClick={() => connect(false)}>
              <IconPlug /> Connect
            </button>
          </div>
        </>
      );
      break;
    case "waiting":
      body = (
        <>
          <div className="state">
            <span className="dot busy" /> Restarting
          </div>
          <div className="note">{status.detail}</div>
          <div className="actions">
            <button className="btn small ghost" onClick={() => connect(false)}>
              Connect now
            </button>
          </div>
        </>
      );
      break;
    case "connecting":
      body = (
        <>
          <div className="state">
            <span className="dot busy" /> Connecting
          </div>
          <div className="note">{status.detail}</div>
        </>
      );
      break;
    case "failed":
      body = (
        <>
          <div className="state">
            <span className="dot bad" /> Not connected
          </div>
          <div className="note" style={{ userSelect: "text" }}>
            {status.message}
          </div>
          <div className="actions">
            <button className="btn small primary" onClick={() => connect(false)}>
              <IconRefresh /> Try again
            </button>
            <button className="btn small ghost" onClick={() => connect(true)}>
              Simulator
            </button>
          </div>
        </>
      );
      break;
    case "connected":
      body = info && (
        <>
          <div className="state">
            <span className="dot ok" /> Connected
            {simulated && <span className="sim-badge">SIMULATOR</span>}
          </div>
          <div className="name" title={info.name}>
            {info.name || "TI-Nspire"}
          </div>
          <div className="model">
            {modelName(info)} · OS {info.os_version}
          </div>
          <Meter label="Storage" used={info.flash_total - info.flash_free} total={info.flash_total} />
          <dl>
            <dt>Battery</dt>
            <dd>{battery(info)}</dd>
          </dl>
          {details && (
            <>
              <Meter label="Memory" used={info.ram_total - info.ram_free} total={info.ram_total} />
              <dl>
                <dt>Boot loaders</dt>
                <dd title={`boot1 ${info.boot1_version}, boot2 ${info.boot2_version}`}>
                  {info.boot1_version} / {info.boot2_version}
                </dd>
                <dt>Product ID</dt>
                <dd title={info.electronic_id} style={{ userSelect: "text" }}>
                  {info.electronic_id || "—"}
                </dd>
                <dt>Screen</dt>
                <dd>
                  {info.lcd_width}×{info.lcd_height}, {info.lcd_bpp}-bit
                </dd>
              </dl>
            </>
          )}
          <div style={{ display: "flex", gap: 4, marginTop: 6 }}>
            <button className="btn small ghost" style={{ paddingLeft: 0 }} onClick={() => setDetails((d) => !d)}>
              {details ? "Fewer details" : "More details"}
            </button>
            <div style={{ flex: 1 }} />
            <button className="btn small ghost icon" title="Refresh the details" onClick={() => api.refreshInfo().catch((e) => app.toast("bad", "Couldn't refresh", errorText(e)))}>
              <IconRefresh />
            </button>
          </div>
          <div className="actions">
            <button className="btn small" onClick={backup} title="Copy everything on the handheld to this computer">
              <IconArchive /> Back up
            </button>
            <button className="btn small" onClick={restore} title="Send a backup folder's documents to the handheld">
              <IconRestore /> Restore
            </button>
            <button className="btn small" onClick={() => installOsFlow(app)} title="Install an OS update (.tco2 / .tcc2)">
              <IconChip /> Install OS
            </button>
            <button className="btn small ghost" onClick={() => api.disconnect()}>
              Disconnect
            </button>
          </div>
        </>
      );
      break;
  }
  return <div className="device">{body}</div>;
}

export function modelName(info: DeviceInfo): string {
  const cas = info.os_extension.endsWith("cc2") || [0x0e, 0x0f, 0x1c].includes(info.hw_type);
  return cas ? "TI-Nspire CX II CAS" : "TI-Nspire CX II";
}
