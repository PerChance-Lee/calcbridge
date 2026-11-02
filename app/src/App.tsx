import { useEffect, useRef } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { DialogHost, JobsBar, Toasts } from "./components/chrome";
import { Sidebar, TABS } from "./components/Sidebar";
import { useApp } from "./store";
import { CreateView } from "./views/CreateView";
import { FilesView } from "./views/FilesView";
import { KeypadView } from "./views/KeypadView";
import { LogView } from "./views/LogView";
import { ScreenView } from "./views/ScreenView";
import { SettingsView } from "./views/SettingsView";
import { ViewerView } from "./views/ViewerView";

export function App() {
  const app = useApp();
  const { tab, setTab, jobs, simulated } = app;
  const running = useRef(false);
  running.current = jobs.some((j) => j.state === "running" || j.state === "queued");

  // window-wide keys: tab switching; keep the webview's reload and find keys from firing
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && /^[1-6]$/.test(e.key)) {
        e.preventDefault();
        setTab(TABS[Number(e.key) - 1]);
      } else if (e.key === "F5" || ((e.ctrlKey || e.metaKey) && (e.key === "r" || e.key === "R" || e.key === "f" || e.key === "p"))) {
        e.preventDefault();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [setTab]);

  // no browser context menu outside text fields
  useEffect(() => {
    const onMenu = (e: MouseEvent) => {
      const t = e.target as HTMLElement;
      if (!t.closest("input, textarea, .log, .code, .summary, .handheld-page")) e.preventDefault();
    };
    window.addEventListener("contextmenu", onMenu);
    return () => window.removeEventListener("contextmenu", onMenu);
  }, []);

  // closing while a transfer runs could leave a half-written file on the handheld
  useEffect(() => {
    const win = getCurrentWindow();
    const un = win.onCloseRequested(async (event) => {
      if (!running.current) return;
      event.preventDefault();
      const ok = await app.confirm(
        "A transfer is still running",
        <p>Quitting now stops it partway, which can leave an incomplete file on the handheld.</p>,
        "Quit anyway",
        true,
      );
      if (ok) await win.destroy();
    });
    return () => {
      un.then((f) => f());
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    getCurrentWindow()
      .setTitle(simulated ? "CalcBridge — simulator" : "CalcBridge")
      .catch(() => {});
  }, [simulated]);

  return (
    <div className="app">
      <Sidebar />
      <main className="main">
        {/* Keep views mounted so their state survives tab switches; hide the inactive ones. */}
        <div style={{ display: tab === "files" ? "contents" : "none" }}>
          <FilesView />
        </div>
        {tab === "screen" && <ScreenView />}
        {tab === "keypad" && <KeypadView />}
        <div style={{ display: tab === "create" ? "contents" : "none" }}>
          <CreateView />
        </div>
        <div style={{ display: tab === "viewer" ? "contents" : "none" }}>
          <ViewerView />
        </div>
        {tab === "log" && <LogView />}
        {tab === "settings" && <SettingsView />}
      </main>
      <JobsBar />
      <Toasts />
      <DialogHost />
    </div>
  );
}
