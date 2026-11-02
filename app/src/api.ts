// Typed access to the Rust side: commands (invoke) and events (listen).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface DeviceInfo {
  name: string;
  os_version: string;
  boot1_version: string;
  boot2_version: string;
  hw_type: number;
  hw_version: number;
  flash_free: number;
  flash_total: number;
  ram_free: number;
  ram_total: number;
  battery: string;
  charging: boolean;
  clock_mhz: number;
  lcd_width: number;
  lcd_height: number;
  lcd_bpp: number;
  run_level: string;
  electronic_id: string;
  file_extension: string;
  os_extension: string;
}

export type Status =
  | { state: "absent" }
  | { state: "present" }
  | { state: "connecting"; detail: string }
  | { state: "connected"; info: DeviceInfo; simulated: boolean }
  | { state: "waiting"; detail: string }
  | { state: "failed"; message: string };

export interface DirEntry {
  name: string;
  size: number;
  is_dir: boolean;
  date: number;
  path: string;
}

export interface LocalEntry {
  name: string;
  path: string;
  isDir: boolean;
  size: number;
  modified: number | null;
}

export interface Places {
  home: string;
  documents: string | null;
  desktop: string | null;
  downloads: string | null;
  roots: string[];
  separator: string;
}

export interface JobEvent {
  id: number;
  title: string;
  state: "queued" | "running" | "done" | "failed" | "cancelled";
  done: number;
  total: number;
  detail: string;
  message: string;
  errorKind: string | null;
}

export interface LogLine {
  time: number;
  level: "error" | "warn" | "info" | "debug";
  message: string;
}

export interface TextStyle {
  bold?: boolean;
  italic?: boolean;
  sub?: boolean;
  sup?: boolean;
  color?: string;
}

export type Span = { kind: "text"; text: string; style: TextStyle } | { kind: "math"; latex: string };
export type ViewParagraph = Span[];

export interface PreviewPage {
  paragraphs: ViewParagraph[];
  errors: [number, string][];
}

export interface DocPage {
  problem: number;
  index: number;
  apps: string[];
}

export interface DocView {
  name: string;
  size: number;
  summary: string;
  pages: DocPage[];
  notes: { problem: number; page: number; paragraphs: ViewParagraph[]; error: string | null }[];
  python: { name: string; code: string }[];
  lua: string | null;
}

export type DocKind = "notes" | "python" | "lua" | "text";

export interface DocSpec {
  kind: DocKind;
  name: string;
  content: string;
  titleColor?: number;
}

export interface UsbDevice {
  product: string;
  serial: string | null;
  bus: string;
  address: number;
}

export interface OsImageCheck {
  size: number;
  wantedExtension: string;
  matches: boolean;
  currentVersion: string;
}

/** What a failed command rejects with. */
export interface CmdError {
  kind: string;
  message: string;
}

export function errorText(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as CmdError).message);
  return String(e);
}

export function errorKind(e: unknown): string | null {
  return e && typeof e === "object" && "kind" in e ? String((e as CmdError).kind) : null;
}

export interface Screenshot {
  width: number;
  height: number;
  bpp: number;
  pixels: Uint8Array;
}

export const api = {
  status: () => invoke<Status>("status"),
  connect: (simulated: boolean) => invoke<DeviceInfo>("connect", { simulated }),
  disconnect: () => invoke<void>("disconnect"),
  refreshInfo: () => invoke<DeviceInfo>("refresh_info"),
  setAutoConnect: (on: boolean) => invoke<void>("set_auto_connect", { on }),
  usbDevices: () => invoke<UsbDevice[]>("usb_devices"),
  logLines: () => invoke<LogLine[]>("log_lines"),

  listDir: (path: string) => invoke<DirEntry[]>("list_dir", { path }),
  exists: (paths: string[]) => invoke<boolean[]>("exists", { paths }),
  makeDir: (path: string) => invoke<void>("make_dir", { path }),
  rename: (path: string, newName: string) => invoke<string>("rename", { path, newName }),
  delete: (paths: string[]) => invoke<number>("delete", { paths }),
  copyOrMove: (paths: string[], folder: string, keepOriginal: boolean) =>
    invoke<number>("copy_or_move", { paths, folder, keepOriginal }),
  upload: (locals: string[], folder: string, existing: "replace" | "skip", convert = false) =>
    invoke<number>("upload", { locals, folder, existing, convert }),
  download: (paths: string[], folder: string) => invoke<number>("download", { paths, folder }),
  backup: (folder: string) => invoke<number>("backup", { folder }),
  restore: (folder: string, target: string, replace: boolean) => invoke<number>("restore", { folder, target, replace }),
  osImageCheck: (path: string) => invoke<OsImageCheck>("os_image_check", { path }),
  installOs: (path: string) => invoke<number>("install_os", { path }),
  cancelJob: (id: number) => invoke<void>("cancel_job", { id }),

  async screenshot(): Promise<Screenshot> {
    const raw = await invoke<ArrayBuffer>("screenshot");
    const view = new DataView(raw);
    return {
      width: view.getUint16(0, true),
      height: view.getUint16(2, true),
      bpp: view.getUint8(4),
      pixels: new Uint8Array(raw, 6),
    };
  },
  saveScreenshot: (path: string) => invoke<void>("save_screenshot", { path }),
  savePng: (path: string, base64: string) => invoke<void>("save_png", { path, base64 }),
  pressKey: (key: string) => invoke<void>("press_key", { key }),
  typeText: (text: string) => invoke<void>("type_text", { text }),

  localList: (path: string, showHidden: boolean) => invoke<LocalEntry[]>("local_list", { path, showHidden }),
  localPlaces: () => invoke<Places>("local_places"),
  localMakeDir: (path: string) => invoke<void>("local_make_dir", { path }),
  localRename: (path: string, newName: string) => invoke<string>("local_rename", { path, newName }),
  localExists: (paths: string[]) => invoke<boolean[]>("local_exists", { paths }),
  localIsDir: (paths: string[]) => invoke<boolean[]>("local_is_dir", { paths }),
  writeTextFile: (path: string, content: string) => invoke<void>("write_text_file", { path, content }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),

  previewNotes: (content: string, titleColor?: number) => invoke<PreviewPage[]>("preview_notes", { content, titleColor }),
  saveDocument: (spec: DocSpec, path: string) => invoke<void>("save_document", { spec, path }),
  sendDocument: (spec: DocSpec, folder: string, replace: boolean) =>
    invoke<string>("send_document", { spec, folder, replace }),
  installModule: (spec: DocSpec, replace: boolean) => invoke<string>("install_module", { spec, replace }),
  openLocalDocument: (path: string) => invoke<DocView>("open_local_document", { path }),
  openHandheldDocument: (path: string) => invoke<DocView>("open_handheld_document", { path }),
  saveCodeFiles: (folder: string, files: [string, string][]) => invoke<string[]>("save_code_files", { folder, files }),
  localNameFor: (name: string) => invoke<string>("local_name_for", { name }),
};

export const on = {
  status: (f: (s: Status) => void): Promise<UnlistenFn> => listen<Status>("status", (e) => f(e.payload)),
  job: (f: (j: JobEvent) => void): Promise<UnlistenFn> => listen<JobEvent>("job", (e) => f(e.payload)),
  log: (f: (l: LogLine) => void): Promise<UnlistenFn> => listen<LogLine>("log", (e) => f(e.payload)),
};

// ------------------------------------------------------------------ helpers
export function formatSize(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(n < 10 * 1024 ? 1 : 0)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`;
}

export function formatDate(secs: number | null): string {
  if (!secs) return "";
  return new Date(secs * 1000).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

/** Handheld paths always use "/". */
export function joinRemote(folder: string, name: string): string {
  const f = folder.replace(/\/+$/, "");
  return `${f}/${name}`;
}

export function parentRemote(path: string): string {
  const p = path.replace(/\/+$/, "");
  const i = p.lastIndexOf("/");
  return i <= 0 ? "/" : p.slice(0, i);
}

export function baseName(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function joinLocal(folder: string, name: string, sep: string): string {
  return folder.endsWith(sep) || folder.endsWith("/") ? folder + name : folder + sep + name;
}

export function parentLocal(path: string, sep: string): string | null {
  const trimmed = path.replace(/[\\/]+$/, "");
  const i = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  if (i < 0) return null;
  const parent = trimmed.slice(0, i);
  if (/^[A-Za-z]:$/.test(parent)) return parent + sep; // C:\
  if (parent === "") return sep === "/" ? "/" : null;
  return parent;
}

/** Turn RGB565 (little endian) or 4-bit grey pixels into canvas ImageData. */
export function toImageData(shot: Screenshot): ImageData {
  const { width, height, bpp, pixels } = shot;
  const img = new ImageData(width, height);
  const out = img.data;
  const n = width * height;
  if (bpp === 16) {
    for (let i = 0; i < n; i++) {
      const v = pixels[2 * i] | (pixels[2 * i + 1] << 8);
      const r = (v >> 11) & 0x1f;
      const g = (v >> 5) & 0x3f;
      const b = v & 0x1f;
      out[4 * i] = (r << 3) | (r >> 2);
      out[4 * i + 1] = (g << 2) | (g >> 4);
      out[4 * i + 2] = (b << 3) | (b >> 2);
      out[4 * i + 3] = 255;
    }
  } else {
    for (let i = 0; i < n; i++) {
      const byte = pixels[i >> 1];
      const nib = i & 1 ? byte & 0xf : byte >> 4;
      const g = nib * 17;
      out[4 * i] = out[4 * i + 1] = out[4 * i + 2] = g;
      out[4 * i + 3] = 255;
    }
  }
  return img;
}
