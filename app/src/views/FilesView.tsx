// Files: this computer on the left, the handheld on the right; send and receive between them.
import { useCallback, useEffect, useRef, useState, type KeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { revealItemInDir } from "@tauri-apps/plugin-opener";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { installOsFlow, isOsImage } from "../flows";
import {
  api, baseName, errorKind, errorText, formatSize, joinLocal, joinRemote, parentLocal, parentRemote, type Places,
} from "../api";
import { FileList, kindOf, type Row } from "../components/FileList";
import { useContextMenu, type MenuEntry } from "../components/chrome";
import {
  IconCalc, IconComputer, IconCopy, IconFolderPlus, IconHome, IconMove, IconOpen, IconPencil, IconReceive, IconRefresh,
  IconReveal, IconSend, IconTrash, IconUp, IconBook, IconCreate, IconInfo,
} from "../icons";
import { useApp } from "../store";

type Side = "local" | "handheld";

interface Drag {
  from: Side;
  rows: Row[];
  x: number;
  y: number;
  over: Side | null;
  folder: string | null;
}

const NAME_RULE = /[\\/:*?"<>|]/;

function checkHandheldName(name: string): string | null {
  const n = name.trim();
  if (!n || n === "." || n === "..") return "The name can't be empty.";
  if (NAME_RULE.test(n)) return 'The handheld doesn\'t allow \\ / : * ? " < > | in names.';
  if (new TextEncoder().encode(n).length > 255) return "That name is too long.";
  return null;
}

function editableKind(name: string): "python" | "lua" | "text" | "notes" | null {
  const n = name.toLowerCase();
  if (n.endsWith(".py")) return "python";
  if (n.endsWith(".lua")) return "lua";
  if (n.endsWith(".md") || n.endsWith(".notes")) return "notes";
  if (n.endsWith(".txt")) return "text";
  return null;
}

export function FilesView() {
  const app = useApp();
  const { connected, settings, updateSettings, handheldVersion, simulated } = app;
  const menu = useContextMenu();

  // ------------------------------------------------------------ this computer
  const [places, setPlaces] = useState<Places | null>(null);
  const [localPath, setLocalPath] = useState<string | null>(settings.localFolder);
  const [localRows, setLocalRows] = useState<Row[]>([]);
  const [localSel, setLocalSel] = useState<Set<string>>(new Set());
  const [localError, setLocalError] = useState<string | null>(null);
  const [localFilter, setLocalFilter] = useState("");
  const [pathEdit, setPathEdit] = useState<string | null>(null);

  // ------------------------------------------------------------ the handheld
  const [remotePath, setRemotePath] = useState(settings.remoteFolder || "/");
  const [remoteRows, setRemoteRows] = useState<Row[]>([]);
  const [remoteSel, setRemoteSel] = useState<Set<string>>(new Set());
  const [remoteError, setRemoteError] = useState<string | null>(null);
  const [remoteFilter, setRemoteFilter] = useState("");
  const [loadingRemote, setLoadingRemote] = useState(false);
  const clip = useRef<{ paths: string[]; keep: boolean } | null>(null);

  const [drag, setDrag] = useState<Drag | null>(null);
  const [nativeOver, setNativeOver] = useState<Side | null>(null);
  const sep = places?.separator ?? "\\";

  useEffect(() => {
    api.localPlaces().then((p) => {
      setPlaces(p);
      setLocalPath((cur) => cur ?? p.documents ?? p.home);
    });
  }, []);

  const localRequest = useRef(0);
  const remoteRequest = useRef(0);

  const loadLocal = useCallback(
    async (path: string | null = localPath) => {
      if (!path) return;
      const ticket = ++localRequest.current;
      try {
        const entries = await api.localList(path, settings.showHidden);
        if (ticket !== localRequest.current) return; // a newer folder was asked for meanwhile
        setLocalRows(entries.map((e) => ({ key: e.path, name: e.name, isDir: e.isDir, size: e.size, modified: e.modified, kind: kindOf(e.name, e.isDir) })));
        setLocalError(null);
      } catch (e) {
        if (ticket !== localRequest.current) return;
        setLocalRows([]);
        setLocalError(errorText(e));
      }
    },
    [localPath, settings.showHidden],
  );

  useEffect(() => {
    loadLocal();
    setLocalSel(new Set());
    if (localPath) updateSettings({ localFolder: localPath });
  }, [localPath, settings.showHidden]); // eslint-disable-line react-hooks/exhaustive-deps

  const loadRemote = useCallback(
    async (path: string = remotePath) => {
      if (!connected) return;
      const ticket = ++remoteRequest.current;
      setLoadingRemote(true);
      try {
        const entries = await api.listDir(path);
        // A listing that waited behind a transfer can finish after a newer one: showing it under the
        // newer folder's breadcrumb would make Delete and Rename act on the wrong folder's files.
        if (ticket !== remoteRequest.current) return;
        setRemoteRows(entries.map((e) => ({ key: e.path, name: e.name, isDir: e.is_dir, size: e.size, modified: null, kind: kindOf(e.name, e.is_dir) })));
        setRemoteError(null);
      } catch (e) {
        if (ticket !== remoteRequest.current) return;
        if (errorKind(e) === "notFound" && path !== "/") {
          setRemotePath("/"); // the folder went away (deleted on the handheld, or a different calculator)
          return;
        }
        setRemoteRows([]);
        setRemoteError(errorText(e));
      } finally {
        if (ticket === remoteRequest.current) setLoadingRemote(false);
      }
    },
    [connected, remotePath],
  );

  useEffect(() => {
    if (!connected) {
      setRemoteRows([]);
      setRemoteSel(new Set());
      return;
    }
    loadRemote();
    updateSettings({ remoteFolder: remotePath });
  }, [connected, remotePath, handheldVersion]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => setRemoteSel(new Set()), [remotePath]);
  useEffect(() => {
    loadLocal(); // a download may have added files here
  }, [handheldVersion]); // eslint-disable-line react-hooks/exhaustive-deps

  // ---------------------------------------------------------------- transfers
  async function send(paths: string[], folder: string) {
    if (!connected) return app.toast("info", "Connect a calculator first");
    if (!paths.length) return;
    // an OS image goes through the install flow instead
    const os = paths.find((p) => isOsImage(baseName(p)));
    if (os) {
      const ok = await app.confirm("Install an OS?", <p><b>{baseName(os)}</b> is an OS file. Install it on the handheld?</p>, "Continue");
      if (ok) await installOsFlow(app, os);
      paths = paths.filter((p) => p !== os && !isOsImage(baseName(p)));
      if (!paths.length) return;
    }
    const isDirFlags = await api.localIsDir(paths).catch(() => paths.map(() => false));
    const dirs = new Set(paths.filter((_, i) => isDirFlags[i]));
    const isDoc = (p: string) => dirs.has(p) || baseName(p).toLowerCase().endsWith(".tns");
    const convertible = paths.filter((p) => !isDoc(p) && editableKind(baseName(p)));
    const other = paths.filter((p) => !isDoc(p) && !editableKind(baseName(p)));
    let convert = false;
    if (convertible.length) {
      const r = await app.ask({
        title: convertible.length === 1 ? `Turn “${baseName(convertible[0])}” into a document?` : `Turn ${convertible.length} files into documents?`,
        text: (
          <p>
            The handheld only accepts TI-Nspire documents (.tns). CalcBridge can convert {convertible.length === 1 ? "it" : "them"}: Python and Lua
            files become programs, text and Markdown files become a Notes page.
          </p>
        ),
        choices: [
          { id: "cancel", label: "Cancel" },
          ...(convertible.length === 1 ? [{ id: "edit", label: "Edit in Create first" }] : []),
          { id: "skip", label: "Leave them out" },
          { id: "convert", label: "Convert and send", primary: true },
        ],
        cancel: "cancel",
      });
      if (r.choice === "cancel") return;
      if (r.choice === "edit") {
        const text = await api.readTextFile(convertible[0]).catch((e) => (app.toast("bad", "Couldn't read it", errorText(e)), null));
        if (text !== null) app.openInCreate(editableKind(baseName(convertible[0]))!, baseName(convertible[0]).replace(/\.[^.]+$/, ""), text);
        return;
      }
      convert = r.choice === "convert";
      if (!convert) paths = paths.filter((p) => !convertible.includes(p));
    }
    if (other.length) {
      app.toast("info", `Left out ${other.length === 1 ? baseName(other[0]) : `${other.length} files`}`, "The handheld only accepts TI-Nspire documents (.tns).");
      paths = paths.filter((p) => !other.includes(p));
    }
    if (!paths.length) return;
    const names = paths.map((p) => (convert && convertible.includes(p) ? baseName(p).replace(/\.[^.]+$/, ".tns") : baseName(p)));
    let existing: "replace" | "skip" = "replace";
    try {
      const there = await api.exists(names.map((n) => joinRemote(folder, n)));
      const clashes = names.filter((_, i) => there[i]);
      if (clashes.length) {
        const r = await app.ask({
          title: clashes.length === 1 ? `“${clashes[0]}” is already on the handheld` : `${clashes.length} items are already on the handheld`,
          text: <p>Replace what's there, or keep it and send only what's new?</p>,
          choices: [
            { id: "cancel", label: "Cancel" },
            { id: "skip", label: "Skip those" },
            { id: "replace", label: "Replace", primary: true },
          ],
          cancel: "cancel",
        });
        if (r.choice === "cancel") return;
        existing = r.choice as "replace" | "skip";
      }
    } catch (e) {
      return app.toast("bad", "Couldn't check the handheld", errorText(e));
    }
    api.upload(paths, folder, existing, convert).catch((e) => app.toast("bad", "Couldn't send", errorText(e)));
  }

  async function receive(paths: string[], folder: string | null) {
    if (!folder || !paths.length) return;
    const targets = await Promise.all(paths.map(async (p) => joinLocal(folder, await api.localNameFor(baseName(p)), sep)));
    const there = await api.localExists(targets);
    const clashes = targets.filter((_, i) => there[i]);
    if (clashes.length) {
      const ok = await app.confirm(
        clashes.length === 1 ? `Replace “${baseName(clashes[0])}”?` : `Replace ${clashes.length} items?`,
        <p>{clashes.length === 1 ? "It's" : "They're"} already in {baseName(folder) || folder} on this computer.</p>,
        "Replace",
        true,
      );
      if (!ok) return;
    }
    api.download(paths, folder).catch((e) => app.toast("bad", "Couldn't receive", errorText(e)));
  }

  // --------------------------------------------------------- handheld actions
  // what the filter hides isn't acted on, even if it was selected before the filter was typed
  const visible = (r: Row, filter: string) => !filter.trim() || r.name.toLowerCase().includes(filter.trim().toLowerCase());
  const remoteSelected = () => remoteRows.filter((r) => remoteSel.has(r.key) && visible(r, remoteFilter));
  const localSelected = () => localRows.filter((r) => localSel.has(r.key) && visible(r, localFilter));

  async function newRemoteFolder() {
    const name = await app.prompt("New folder on the handheld", "New folder", { label: "Name", validate: checkHandheldName, ok: "Create" });
    if (name === null) return;
    try {
      await api.makeDir(joinRemote(remotePath, name.trim()));
      await loadRemote();
      setRemoteSel(new Set([joinRemote(remotePath, name.trim())]));
    } catch (e) {
      app.toast("bad", "Couldn't create the folder", errorText(e));
    }
  }

  async function renameRemote(row: Row) {
    const name = await app.prompt(`Rename “${row.name}”`, row.name, {
      label: "New name",
      validate: (v) => checkHandheldName(v) ?? (v.trim() === row.name ? "That's the current name." : null),
      selectStem: !row.isDir,
      ok: "Rename",
    });
    if (name === null) return;
    let newName = name.trim();
    // the handheld only lists .tns documents, so keep the extension
    if (row.name.toLowerCase().endsWith(".tns") && !newName.toLowerCase().endsWith(".tns")) newName += ".tns";
    try {
      const to = await api.rename(row.key, newName);
      await loadRemote();
      setRemoteSel(new Set([to]));
    } catch (e) {
      app.toast("bad", "Couldn't rename", errorText(e));
    }
  }

  async function deleteRemote(rows: Row[]) {
    if (!rows.length) return;
    if (settings.confirmDelete) {
      const folders = rows.filter((r) => r.isDir).length;
      const what = rows.length === 1 ? `“${rows[0].name}”` : `${rows.length} items`;
      const ok = await app.confirm(
        `Delete ${what} from the handheld?`,
        <p>
          {folders > 0 ? "Folders are deleted with everything in them. " : ""}This can't be undone; the handheld has no recycle bin.
        </p>,
        "Delete",
        true,
      );
      if (!ok) return;
    }
    api.delete(rows.map((r) => r.key)).catch((e) => app.toast("bad", "Couldn't delete", errorText(e)));
  }

  function paste(folder: string) {
    const c = clip.current;
    if (!c) return;
    if (c.paths.some((p) => folder === p || folder.startsWith(`${p}/`))) {
      return app.toast("info", "Can't paste a folder into itself");
    }
    api.copyOrMove(c.paths, folder, c.keep).catch((e) => app.toast("bad", "Couldn't paste", errorText(e)));
    if (!c.keep) clip.current = null;
  }

  function duplicate(rows: Row[]) {
    api.copyOrMove(rows.map((r) => r.key), remotePath, true).catch((e) => app.toast("bad", "Couldn't duplicate", errorText(e)));
  }

  function properties(row: Row) {
    app.ask({
      title: row.name,
      tone: "info",
      text: (
        <div className="syntax" style={{ gridTemplateColumns: "auto 1fr", userSelect: "text" }}>
          <span>Location</span>
          <span>{row.key}</span>
          <span>Type</span>
          <span>{row.kind}</span>
          {!row.isDir && (
            <>
              <span>Size</span>
              <span>
                {formatSize(row.size)} ({row.size.toLocaleString()} bytes)
              </span>
            </>
          )}
        </div>
      ),
      choices: [{ id: "ok", label: "OK", primary: true }],
      cancel: "ok",
    });
  }

  async function pickAndSend() {
    const picked = await openDialog({
      title: "Send files to the handheld",
      multiple: true,
      defaultPath: localPath ?? undefined,
      filters: [
        { name: "TI-Nspire documents", extensions: ["tns"] },
        { name: "Files CalcBridge can convert", extensions: ["py", "lua", "txt", "md"] },
        { name: "All files", extensions: ["*"] },
      ],
    });
    if (Array.isArray(picked) && picked.length) send(picked, remotePath);
    else if (typeof picked === "string") send([picked], remotePath);
  }

  function openRemote(row: Row) {
    if (row.isDir) setRemotePath(row.key);
    else if (row.name.toLowerCase().endsWith(".tns")) app.openInViewer("handheld", row.key);
  }

  function remoteKey(e: KeyboardEvent) {
    const rows = remoteSelected();
    if (e.key === "Delete") deleteRemote(rows);
    else if (e.key === "F2" && rows.length === 1) renameRemote(rows[0]);
    else if (e.key === "Backspace" && remotePath !== "/") setRemotePath(parentRemote(remotePath));
    else if (e.key === "F5") {
      loadRemote();
      api.refreshInfo().catch(() => {});
    } else if ((e.ctrlKey || e.metaKey) && e.shiftKey && (e.key === "D" || e.key === "d") && rows.length) duplicate(rows);
    else if ((e.ctrlKey || e.metaKey) && (e.key === "c" || e.key === "x") && rows.length) {
      clip.current = { paths: rows.map((r) => r.key), keep: e.key === "c" };
      app.toast("info", `${e.key === "c" ? "Copied" : "Cut"} ${rows.length === 1 ? rows[0].name : `${rows.length} items`}`, "Paste them into another handheld folder with Ctrl+V.");
    } else if ((e.ctrlKey || e.metaKey) && e.key === "v") paste(remotePath);
    else return;
    e.preventDefault();
  }

  function remoteMenu(e: React.MouseEvent, row: Row | null) {
    const rows = row ? (remoteSel.has(row.key) ? remoteSelected() : [row]) : [];
    const items: MenuEntry[] = [];
    if (row && rows.length === 1) {
      if (row.isDir) items.push({ label: "Open", icon: <IconOpen />, action: () => openRemote(row) });
      else if (row.name.toLowerCase().endsWith(".tns")) items.push({ label: "View document", icon: <IconBook />, action: () => openRemote(row) });
    }
    if (rows.length) {
      items.push({ label: `Receive to ${localPath ? baseName(localPath) || localPath : "this computer"}`, icon: <IconReceive />, action: () => receive(rows.map((r) => r.key), localPath) });
      items.push("-");
      items.push({ label: "Copy", icon: <IconCopy />, kbd: "Ctrl+C", action: () => (clip.current = { paths: rows.map((r) => r.key), keep: true }) });
      items.push({ label: "Cut", icon: <IconMove />, kbd: "Ctrl+X", action: () => (clip.current = { paths: rows.map((r) => r.key), keep: false }) });
      items.push({ label: "Duplicate", kbd: "Ctrl+Shift+D", action: () => duplicate(rows) });
    }
    items.push({ label: row?.isDir && rows.length === 1 ? `Paste into “${row.name}”` : "Paste", kbd: "Ctrl+V", disabled: !clip.current, action: () => paste(row?.isDir && rows.length === 1 ? row.key : remotePath) });
    items.push("-");
    if (rows.length === 1) items.push({ label: "Rename", icon: <IconPencil />, kbd: "F2", action: () => renameRemote(rows[0]) });
    if (rows.length) items.push({ label: "Delete", icon: <IconTrash />, kbd: "Del", danger: true, action: () => deleteRemote(rows) });
    items.push({ label: "New folder", icon: <IconFolderPlus />, action: newRemoteFolder });
    items.push({ label: "Refresh", icon: <IconRefresh />, kbd: "F5", action: () => loadRemote() });
    if (rows.length === 1) {
      items.push("-");
      items.push({ label: "Properties", icon: <IconInfo />, action: () => properties(rows[0]) });
    }
    menu.open(e, items);
  }

  // ----------------------------------------------------------- local actions
  async function newLocalFolder() {
    if (!localPath) return;
    const name = await app.prompt("New folder", "New folder", { label: "Name", ok: "Create" });
    if (name === null || !name.trim()) return;
    try {
      const path = joinLocal(localPath, name.trim(), sep);
      await api.localMakeDir(path);
      await loadLocal();
      setLocalSel(new Set([path]));
    } catch (e) {
      app.toast("bad", "Couldn't create the folder", errorText(e));
    }
  }

  async function renameLocal(row: Row) {
    const name = await app.prompt(`Rename “${row.name}”`, row.name, { label: "New name", selectStem: !row.isDir, ok: "Rename" });
    if (name === null || name.trim() === row.name) return;
    try {
      const to = await api.localRename(row.key, name);
      await loadLocal();
      setLocalSel(new Set([to]));
    } catch (e) {
      app.toast("bad", "Couldn't rename", errorText(e));
    }
  }

  async function openLocal(row: Row) {
    if (row.isDir) return setLocalPath(row.key);
    const lower = row.name.toLowerCase();
    if (lower.endsWith(".tns")) return app.openInViewer("local", row.key);
    const kind = editableKind(row.name);
    if (kind) {
      try {
        const text = await api.readTextFile(row.key);
        app.openInCreate(kind, row.name.replace(/\.[^.]+$/, ""), text);
      } catch (e) {
        app.toast("bad", "Couldn't open it", errorText(e));
      }
    }
  }

  function localKey(e: KeyboardEvent) {
    const rows = localSelected();
    if (e.key === "F2" && rows.length === 1) renameLocal(rows[0]);
    else if (e.key === "Backspace" && localPath) {
      const up = parentLocal(localPath, sep);
      if (up) setLocalPath(up);
    } else return;
    e.preventDefault();
  }

  function localMenu(e: React.MouseEvent, row: Row | null) {
    const rows = row ? (localSel.has(row.key) ? localSelected() : [row]) : [];
    const items: MenuEntry[] = [];
    if (row && rows.length === 1) {
      if (row.isDir) items.push({ label: "Open", icon: <IconOpen />, action: () => openLocal(row) });
      else if (row.name.toLowerCase().endsWith(".tns")) items.push({ label: "View document", icon: <IconBook />, action: () => openLocal(row) });
      else if (editableKind(row.name)) items.push({ label: "Turn into a .tns in Create", icon: <IconCreate />, action: () => openLocal(row) });
    }
    if (rows.length) {
      items.push({ label: `Send to the handheld (${remotePath})`, icon: <IconSend />, disabled: !connected, action: () => send(rows.map((r) => r.key), remotePath) });
      items.push("-");
      if (rows.length === 1) items.push({ label: "Rename", icon: <IconPencil />, kbd: "F2", action: () => renameLocal(rows[0]) });
      items.push({ label: navigator.platform.startsWith("Mac") ? "Show in Finder" : "Show in folder", icon: <IconReveal />, action: () => revealItemInDir(rows[0].key) });
    }
    items.push({ label: "New folder", icon: <IconFolderPlus />, action: newLocalFolder });
    items.push({ label: "Refresh", icon: <IconRefresh />, action: () => loadLocal() });
    menu.open(e, items);
  }

  // ------------------------------------------------------------ drag & drop
  function sideAt(x: number, y: number): { side: Side | null; folder: string | null } {
    const el = document.elementFromPoint(x, y) as HTMLElement | null;
    const pane = el?.closest<HTMLElement>("[data-pane]");
    const side = (pane?.dataset.pane as Side | undefined) ?? null;
    const tr = el?.closest<HTMLElement>("tr[data-key]");
    const key = tr?.dataset.key ?? null;
    const rows = side === "local" ? localRows : remoteRows;
    const folder = key && rows.some((r) => r.key === key && r.isDir) ? key : null;
    return { side, folder };
  }

  function startDrag(from: Side, rows: Row[], e: ReactPointerEvent) {
    setDrag({ from, rows, x: e.clientX, y: e.clientY, over: from, folder: null });
  }

  useEffect(() => {
    if (!drag) return;
    const move = (e: PointerEvent) => {
      const { side, folder } = sideAt(e.clientX, e.clientY);
      const dragged = new Set(drag.rows.map((r) => r.key));
      setDrag((d) => d && { ...d, x: e.clientX, y: e.clientY, over: side, folder: folder && !dragged.has(folder) ? folder : null });
    };
    const up = (e: PointerEvent) => {
      const { side, folder } = sideAt(e.clientX, e.clientY);
      const d = drag;
      setDrag(null);
      const paths = d.rows.map((r) => r.key);
      if (folder && paths.includes(folder)) return;
      if (d.from === "local" && side === "handheld") send(paths, folder ?? remotePath);
      else if (d.from === "handheld" && side === "local") receive(paths, folder ?? localPath);
      else if (d.from === "handheld" && side === "handheld" && folder) {
        api.copyOrMove(paths, folder, e.ctrlKey).catch((err) => app.toast("bad", "Couldn't move", errorText(err)));
      }
    };
    const cancel = (e: globalThis.KeyboardEvent) => e.key === "Escape" && setDrag(null);
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up, { once: true });
    window.addEventListener("keydown", cancel);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("keydown", cancel);
    };
  }, [drag?.from, drag?.rows]); // eslint-disable-line react-hooks/exhaustive-deps

  // Files dragged in from Explorer / Finder. Subscribe once; the handler reads this render's state.
  const nativeDrop = useRef<(type: string, paths: string[], x: number, y: number) => void>(() => {});
  nativeDrop.current = (type, paths, x, y) => {
    if (type === "leave") return setNativeOver(null);
    const { side, folder } = sideAt(x, y);
    if (type === "over" || type === "enter") return setNativeOver(side);
    if (type !== "drop") return;
    setNativeOver(null);
    if (side === "handheld") send(paths, folder ?? remotePath);
    else if (side === "local") app.toast("info", "Drop them on the handheld side", "Files dropped on the handheld's list are sent to it.");
  };
  useEffect(() => {
    const un = getCurrentWebview().onDragDropEvent((event) => {
      const p = event.payload;
      const scale = window.devicePixelRatio || 1;
      const pos = "position" in p ? p.position : { x: 0, y: 0 };
      nativeDrop.current(p.type, "paths" in p ? p.paths : [], pos.x / scale, pos.y / scale);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  // ------------------------------------------------------------------ render
  const localCrumbs = (() => {
    if (!localPath) return [];
    const parts: { label: string; path: string }[] = [];
    let p: string | null = localPath;
    while (p) {
      parts.unshift({ label: baseName(p) || p, path: p });
      p = parentLocal(p, sep);
    }
    return parts;
  })();
  const remoteCrumbs = [{ label: "My Documents", path: "/" }, ...remotePath.split("/").filter(Boolean).map((part, i, all) => ({ label: part, path: `/${all.slice(0, i + 1).join("/")}` }))];
  const dropSide = drag?.over && drag.over !== drag.from ? drag.over : drag?.folder ? drag.over : nativeOver;

  return (
    <div className="view">
      <div className="panes">
        {/* ------------------------------------------------ this computer */}
        <section className={`pane${dropSide === "local" ? " drop-target" : ""}`} data-pane="local">
          <div className="pane-head">
            <div className="title">
              <IconComputer /> This computer
            </div>
            <div className="grow" />
            <input type="search" className="filter" placeholder="Filter" value={localFilter} onChange={(e) => setLocalFilter(e.target.value)} />
          </div>
          <div className="pane-tools">
            <button className="btn small icon ghost" title="Up (Backspace)" disabled={!localPath || !parentLocal(localPath, sep)} onClick={() => localPath && setLocalPath(parentLocal(localPath, sep))}>
              <IconUp />
            </button>
            <button
              className="btn small icon ghost"
              title="Places"
              onClick={(e) => {
                if (!places) return;
                const items: MenuEntry[] = [
                  { label: "Home", icon: <IconHome />, action: () => setLocalPath(places.home) },
                  ...(places.documents ? [{ label: "Documents", icon: <IconOpen />, action: () => setLocalPath(places.documents) }] : []),
                  ...(places.desktop ? [{ label: "Desktop", icon: <IconOpen />, action: () => setLocalPath(places.desktop) }] : []),
                  ...(places.downloads ? [{ label: "Downloads", icon: <IconOpen />, action: () => setLocalPath(places.downloads) }] : []),
                  "-" as const,
                  ...places.roots.map((r) => ({ label: r, icon: <IconComputer />, action: () => setLocalPath(r) })),
                ];
                const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
                menu.open({ clientX: rect.left, clientY: rect.bottom + 4, preventDefault() {} }, items);
              }}
            >
              <IconHome />
            </button>
            {pathEdit !== null ? (
              <input
                autoFocus
                className="path-input"
                value={pathEdit}
                spellCheck={false}
                onChange={(e) => setPathEdit(e.target.value)}
                onBlur={() => setPathEdit(null)}
                onKeyDown={async (e) => {
                  if (e.key === "Escape") setPathEdit(null);
                  if (e.key !== "Enter") return;
                  const target = pathEdit.trim();
                  const [isDir] = await api.localIsDir([target]);
                  if (isDir) {
                    setLocalPath(target);
                    setPathEdit(null);
                  } else app.toast("info", "No such folder", target);
                }}
              />
            ) : (
            <div className="crumbs" onDoubleClick={(e) => e.target === e.currentTarget && setPathEdit(localPath ?? "")} title="Double-click to type a path">
              {localCrumbs.length > 3 && (
                <button
                  title="Folders above"
                  onClick={(e) => {
                    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
                    menu.open(
                      { clientX: rect.left, clientY: rect.bottom + 4, preventDefault() {} },
                      localCrumbs.slice(0, -2).map((c) => ({ label: c.path, icon: <IconOpen />, action: () => setLocalPath(c.path) })),
                    );
                  }}
                >
                  …
                </button>
              )}
              {localCrumbs.slice(localCrumbs.length > 3 ? -2 : 0).map((c, i) => (
                <span key={c.path} style={{ display: "contents" }}>
                  {(i > 0 || localCrumbs.length > 3) && <span className="chev">›</span>}
                  <button onClick={() => setLocalPath(c.path)} title={c.path}>
                    {c.label}
                  </button>
                </span>
              ))}
              <span style={{ flex: 1, alignSelf: "stretch" }} onClick={() => setPathEdit(localPath ?? "")} title="Click to type a path" />
            </div>
            )}
            <button className="btn small icon ghost" title="New folder" onClick={newLocalFolder} disabled={!localPath}>
              <IconFolderPlus />
            </button>
            <button className="btn small icon ghost" title="Refresh" onClick={() => loadLocal()}>
              <IconRefresh />
            </button>
            <button
              className="btn small primary"
              disabled={!connected || localSel.size === 0}
              onClick={() => send(localSelected().map((r) => r.key), remotePath)}
              title="Send the selection to the open handheld folder"
            >
              Send <IconSend />
            </button>
          </div>
          {localError ? (
            <div className="pane-empty">
              <p>{localError}</p>
              <button className="btn" onClick={() => places && setLocalPath(places.home)}>
                Go home
              </button>
            </div>
          ) : (
            <FileList
              rows={localRows}
              selected={localSel}
              onSelect={setLocalSel}
              onOpen={openLocal}
              onContext={localMenu}
              onKey={localKey}
              onDragStart={(rows, e) => startDrag("local", rows, e)}
              dropKey={drag?.over === "local" ? drag.folder : null}
              showDate
              filter={localFilter}
              empty={<p>{localFilter ? "Nothing matches the filter." : "This folder is empty."}</p>}
            />
          )}
          <div className="pane-foot">
            <span>{localRows.length} items</span>
            {localSel.size > 0 && <span>{localSel.size} selected</span>}
          </div>
        </section>

        {/* ------------------------------------------------ the handheld */}
        <section className={`pane${dropSide === "handheld" ? " drop-target" : ""}`} data-pane="handheld">
          <div className="pane-head">
            <div className="title">
              <IconCalc /> Handheld{simulated && <span className="sim-badge">SIMULATOR</span>}
            </div>
            <div className="grow" />
            <input type="search" className="filter" placeholder="Filter" value={remoteFilter} onChange={(e) => setRemoteFilter(e.target.value)} disabled={!connected} />
          </div>
          <div className="pane-tools">
            <button className="btn small icon ghost" title="Up (Backspace)" disabled={!connected || remotePath === "/"} onClick={() => setRemotePath(parentRemote(remotePath))}>
              <IconUp />
            </button>
            <div className="crumbs">
              {remoteCrumbs.map((c, i) => (
                <span key={c.path} style={{ display: "contents" }}>
                  {i > 0 && <span className="chev">›</span>}
                  <button onClick={() => setRemotePath(c.path)} disabled={!connected}>
                    {c.label}
                  </button>
                </span>
              ))}
            </div>
            {loadingRemote && <span className="spinner" />}
            <button className="btn small icon ghost" title="Send files from this computer…" onClick={pickAndSend} disabled={!connected}>
              <IconSend />
            </button>
            <button className="btn small icon ghost" title="New folder" onClick={newRemoteFolder} disabled={!connected}>
              <IconFolderPlus />
            </button>
            <button className="btn small icon ghost" title="Rename (F2)" onClick={() => remoteSelected().length === 1 && renameRemote(remoteSelected()[0])} disabled={!connected || remoteSel.size !== 1}>
              <IconPencil />
            </button>
            <button className="btn small icon ghost" title="Delete (Del)" onClick={() => deleteRemote(remoteSelected())} disabled={!connected || remoteSel.size === 0}>
              <IconTrash />
            </button>
            <button className="btn small icon ghost" title="Refresh (F5)" onClick={() => loadRemote()} disabled={!connected}>
              <IconRefresh />
            </button>
            <button className="btn small primary" disabled={!connected || remoteSel.size === 0 || !localPath} onClick={() => receive(remoteSelected().map((r) => r.key), localPath)} title="Copy the selection into the open folder on this computer">
              <IconReceive /> Receive
            </button>
          </div>
          {!connected ? (
            <div className="pane-empty">
              <IconCalc />
              <p>Connect a TI-Nspire CX II to see its files.</p>
              <div style={{ display: "flex", gap: 8 }}>
                <button className="btn primary" onClick={() => api.connect(false).catch((e) => app.toast("bad", "Couldn't connect", errorText(e)))}>
                  Connect
                </button>
                <button className="btn" onClick={() => api.connect(true).catch((e) => app.toast("bad", "Couldn't start the simulator", errorText(e)))}>
                  Try the simulator
                </button>
              </div>
            </div>
          ) : remoteError ? (
            <div className="pane-empty">
              <p>{remoteError}</p>
              <button className="btn" onClick={() => setRemotePath("/")}>
                Back to My Documents
              </button>
            </div>
          ) : (
            <FileList
              rows={remoteRows}
              selected={remoteSel}
              onSelect={setRemoteSel}
              onOpen={openRemote}
              onContext={remoteMenu}
              onKey={remoteKey}
              onDragStart={(rows, e) => startDrag("handheld", rows, e)}
              dropKey={drag?.over === "handheld" ? drag.folder : null}
              showDate={false}
              filter={remoteFilter}
              empty={<p>{remoteFilter ? "Nothing matches the filter." : "This folder is empty. Drop .tns documents here to send them."}</p>}
            />
          )}
          <div className="pane-foot">
            {connected && <span>{remoteRows.length} items</span>}
            {remoteSel.size > 0 && <span>{remoteSel.size} selected</span>}
          </div>
        </section>
      </div>
      {drag && drag.over && (drag.over !== drag.from || drag.folder) && (
        <div className="drag-ghost" style={{ left: drag.x, top: drag.y }}>
          {drag.from === "local" && drag.over === "handheld"
            ? `Send ${drag.rows.length === 1 ? drag.rows[0].name : `${drag.rows.length} items`}`
            : drag.from === "handheld" && drag.over === "local"
              ? `Receive ${drag.rows.length === 1 ? drag.rows[0].name : `${drag.rows.length} items`}`
              : `Move into ${baseName(drag.folder ?? "")} (Ctrl copies)`}
        </div>
      )}
      {menu.element}
    </div>
  );
}
