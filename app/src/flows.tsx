// Multi-step flows used from more than one place.
import { open } from "@tauri-apps/plugin-dialog";
import { api, baseName, errorText, formatSize } from "./api";
import type { useApp } from "./store";

type App = ReturnType<typeof useApp>;

export const OS_EXTENSIONS = ["tco2", "tcc2", "tct2", "tco", "tcc", "tct"];

export function isOsImage(name: string): boolean {
  const ext = name.toLowerCase().split(".").pop() ?? "";
  return OS_EXTENSIONS.includes(ext);
}

/** Pick (unless given) an OS image, check it against the handheld, warn plainly, then install. */
export async function installOsFlow(app: App, file?: string) {
  if (!file) {
    const picked = await open({
      title: "Choose an OS file",
      defaultPath: app.settings.localFolder ?? undefined,
      filters: [{ name: "TI-Nspire OS", extensions: OS_EXTENSIONS }],
    });
    if (typeof picked !== "string") return;
    file = picked;
  }
  let check;
  try {
    check = await api.osImageCheck(file);
  } catch (e) {
    app.toast("bad", "Couldn't check that file", errorText(e));
    return;
  }
  if (!check.matches) {
    await app.ask({
      title: "Wrong OS file",
      tone: "danger",
      text: (
        <p>
          This handheld needs a <b>{check.wantedExtension}</b> OS file, not <b>{baseName(file)}</b>. (CX II: .tco2, CX II CAS: .tcc2, CX II-T:
          .tct2.)
        </p>
      ),
      choices: [{ id: "ok", label: "OK", primary: true }],
      cancel: "ok",
    });
    return;
  }
  const r = await app.ask({
    title: "Install this OS?",
    tone: "warn",
    wide: true,
    text: (
      <>
        <p>
          <b>{baseName(file)}</b> ({formatSize(check.size)}) goes to {app.info?.name ? <b>{app.info.name}</b> : "the handheld"}, which is
          on OS {check.currentVersion} now. The handheld checks that it's a genuine TI OS, installs it and restarts.
        </p>
        <div className="callout warn" style={{ marginTop: 10 }}>
          Unsaved work on the handheld will be lost. Keep the cable connected until it has restarted. Back up first: CalcBridge's OS install
          follows libnspire's protocol but hasn't been tested on every OS version.
        </div>
      </>
    ),
    checkbox: "I've backed up and want to install this OS",
    choices: [
      { id: "cancel", label: "Cancel" },
      { id: "ok", label: "Install", danger: true },
    ],
    cancel: "cancel",
  });
  if (r.choice !== "ok") return;
  if (!r.checked) {
    app.toast("info", "Not installed", "Tick the confirmation box to install an OS.");
    return;
  }
  await api.installOs(file).catch((e) => app.toast("bad", "OS install failed", errorText(e)));
}
