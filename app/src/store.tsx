// App-wide state: connection status, jobs, settings, toasts, dialogs and cross-tab requests.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, on, type DeviceInfo, type DocKind, type JobEvent, type Status } from "./api";

export type Tab = "files" | "screen" | "keypad" | "create" | "viewer" | "log" | "settings";

export interface Settings {
  theme: "system" | "light" | "dark";
  autoConnect: boolean;
  showHidden: boolean;
  confirmDelete: boolean;
  liveFps: number;
  smoothScreen: boolean;
  localFolder: string | null;
  remoteFolder: string;
  createFolder: string;
}

const DEFAULTS: Settings = {
  theme: "system",
  autoConnect: true,
  showHidden: false,
  confirmDelete: true,
  liveFps: 10,
  smoothScreen: false,
  localFolder: null,
  remoteFolder: "/",
  createFolder: "/",
};

const KEY = "calcbridge.settings";

function loadSettings(): Settings {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return DEFAULTS;
  }
}

export interface Toast {
  id: number;
  kind: "ok" | "bad" | "info";
  title: string;
  text?: string;
}

export interface ViewerRequest {
  source: "local" | "handheld";
  path: string;
  nonce: number;
}

export interface CreateRequest {
  kind: DocKind;
  name: string;
  content: string;
  nonce: number;
}

// ------------------------------------------------------------------ dialogs
export interface Choice {
  id: string;
  label: string;
  primary?: boolean;
  danger?: boolean;
}

export interface DialogSpec {
  title: string;
  text?: ReactNode;
  tone?: "warn" | "danger" | "info";
  /** Show a text field and resolve with its value. */
  input?: { value: string; label?: string; hint?: string; validate?: (v: string) => string | null; selectStem?: boolean };
  choices: Choice[];
  /** The choice Escape picks. */
  cancel?: string;
  /** A checkbox; its state comes back in `checked`. */
  checkbox?: string;
  wide?: boolean;
}

export interface DialogResult {
  choice: string;
  value: string;
  checked: boolean;
}

interface Pending extends DialogSpec {
  resolve: (r: DialogResult) => void;
}

interface Ctx {
  status: Status;
  info: DeviceInfo | null;
  connected: boolean;
  simulated: boolean;
  tab: Tab;
  setTab: (t: Tab) => void;
  settings: Settings;
  updateSettings: (patch: Partial<Settings>) => void;
  jobs: JobEvent[];
  /** Bumped whenever a job that may have changed the handheld finishes. */
  handheldVersion: number;
  bumpHandheld: () => void;
  toasts: Toast[];
  toast: (kind: Toast["kind"], title: string, text?: string) => void;
  dismissToast: (id: number) => void;
  dialog: Pending | null;
  ask: (spec: DialogSpec) => Promise<DialogResult>;
  confirm: (title: string, text: ReactNode, okLabel: string, danger?: boolean) => Promise<boolean>;
  prompt: (title: string, value: string, opts?: Partial<NonNullable<DialogSpec["input"]>> & { ok?: string; text?: ReactNode }) => Promise<string | null>;
  viewerRequest: ViewerRequest | null;
  openInViewer: (source: "local" | "handheld", path: string) => void;
  createRequest: CreateRequest | null;
  openInCreate: (kind: DocKind, name: string, content: string) => void;
}

const AppContext = createContext<Ctx | null>(null);

export function useApp(): Ctx {
  const ctx = useContext(AppContext);
  if (!ctx) throw new Error("useApp outside AppProvider");
  return ctx;
}

const FINISHED = new Set(["done", "failed", "cancelled"]);

export function AppProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<Status>({ state: "absent" });
  const [tab, setTab] = useState<Tab>("files");
  const [settings, setSettings] = useState<Settings>(loadSettings);
  const [jobs, setJobs] = useState<JobEvent[]>([]);
  const [handheldVersion, setHandheldVersion] = useState(0);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [queue, setQueue] = useState<Pending[]>([]);
  const [viewerRequest, setViewerRequest] = useState<ViewerRequest | null>(null);
  const [createRequest, setCreateRequest] = useState<CreateRequest | null>(null);
  const toastId = useRef(1);
  const nonce = useRef(1);

  const toast = useCallback((kind: Toast["kind"], title: string, text?: string) => {
    const id = toastId.current++;
    setToasts((t) => [...t.slice(-4), { id, kind, title, text }]);
    window.setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), kind === "bad" ? 12000 : 5000);
  }, []);
  const dismissToast = useCallback((id: number) => setToasts((t) => t.filter((x) => x.id !== id)), []);

  // connection status
  useEffect(() => {
    let alive = true;
    api.status().then((s) => alive && setStatus(s));
    const un = on.status((s) => setStatus(s));
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, []);

  // jobs
  useEffect(() => {
    const un = on.job((j) => {
      setJobs((all) => {
        const rest = all.filter((x) => x.id !== j.id);
        const next = [...rest, j].sort((a, b) => a.id - b.id);
        return next.filter((x) => !FINISHED.has(x.state) || x.id === j.id).slice(-50);
      });
      if (FINISHED.has(j.state)) {
        setHandheldVersion((v) => v + 1);
        if (j.state === "done") toast("ok", j.title.replace(/…$/, ""), j.message);
        else if (j.state === "failed") toast("bad", `${j.title} failed`, j.message);
        else toast("info", `${j.title}: cancelled`);
        window.setTimeout(() => setJobs((all) => all.filter((x) => x.id !== j.id)), 400);
      }
    });
    return () => {
      un.then((f) => f());
    };
  }, [toast]);

  // settings: persist, apply theme, tell the backend about auto-connect
  useEffect(() => {
    localStorage.setItem(KEY, JSON.stringify(settings));
    api.setAutoConnect(settings.autoConnect).catch(() => {});
  }, [settings]);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = settings.theme === "dark" || (settings.theme === "system" && media.matches);
      document.documentElement.dataset.theme = dark ? "dark" : "light";
    };
    apply();
    media.addEventListener("change", apply);
    return () => media.removeEventListener("change", apply);
  }, [settings.theme]);

  const updateSettings = useCallback((patch: Partial<Settings>) => setSettings((s) => ({ ...s, ...patch })), []);

  const ask = useCallback(
    (spec: DialogSpec) =>
      new Promise<DialogResult>((resolve) => {
        const pending: Pending = {
          ...spec,
          resolve: (r) => {
            setQueue((q) => q.filter((p) => p !== pending));
            resolve(r);
          },
        };
        setQueue((q) => [...q, pending]);
      }),
    [],
  );

  const confirm = useCallback(
    async (title: string, text: ReactNode, okLabel: string, danger = false) => {
      const r = await ask({
        title,
        text,
        tone: danger ? "danger" : undefined,
        choices: [
          { id: "cancel", label: "Cancel" },
          { id: "ok", label: okLabel, primary: !danger, danger },
        ],
        cancel: "cancel",
      });
      return r.choice === "ok";
    },
    [ask],
  );

  const prompt = useCallback<Ctx["prompt"]>(
    async (title, value, opts = {}) => {
      const r = await ask({
        title,
        text: opts.text,
        input: { value, label: opts.label, hint: opts.hint, validate: opts.validate, selectStem: opts.selectStem },
        choices: [
          { id: "cancel", label: "Cancel" },
          { id: "ok", label: opts.ok ?? "OK", primary: true },
        ],
        cancel: "cancel",
      });
      return r.choice === "ok" ? r.value : null;
    },
    [ask],
  );

  const openInViewer = useCallback((source: "local" | "handheld", path: string) => {
    setViewerRequest({ source, path, nonce: nonce.current++ });
    setTab("viewer");
  }, []);

  const openInCreate = useCallback((kind: DocKind, name: string, content: string) => {
    setCreateRequest({ kind, name, content, nonce: nonce.current++ });
    setTab("create");
  }, []);

  const value = useMemo<Ctx>(() => {
    const connected = status.state === "connected";
    return {
      status,
      info: status.state === "connected" ? status.info : null,
      connected,
      simulated: status.state === "connected" && status.simulated,
      tab,
      setTab,
      settings,
      updateSettings,
      jobs,
      handheldVersion,
      bumpHandheld: () => setHandheldVersion((v) => v + 1),
      toasts,
      toast,
      dismissToast,
      dialog: queue[0] ?? null,
      ask,
      confirm,
      prompt,
      viewerRequest,
      openInViewer,
      createRequest,
      openInCreate,
    };
  }, [status, tab, settings, updateSettings, jobs, handheldVersion, toasts, toast, dismissToast, queue, ask, confirm, prompt, viewerRequest, openInViewer, createRequest, openInCreate]);

  return <AppContext.Provider value={value}>{children}</AppContext.Provider>;
}
