// Viewer: read a .tns document from this computer or the handheld: Notes with their math, Python and
// Lua code, and what's inside.
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, baseName, errorText, formatSize, type DocView } from "../api";
import { Paragraphs } from "../components/Math";
import { IconBook, IconCreate, IconOpen, IconSave, IconSend } from "../icons";
import { useApp } from "../store";

type Item =
  | { kind: "page"; problem: number; index: number; apps: string[] }
  | { kind: "python"; name: string }
  | { kind: "lua" }
  | { kind: "summary" };

const KEYWORDS = new Set(
  "False None True and as assert async await break class continue def del elif else except finally for from global if import in is lambda nonlocal not or pass raise return try while with yield".split(" "),
);

/** A light Python highlighter: keywords, strings, comments, numbers, called names. */
function highlight(line: string): ReactNode[] {
  const out: ReactNode[] = [];
  const re = /(#.*$)|("""|'''|"(?:\\.|[^"\\])*"?|'(?:\\.|[^'\\])*'?)|(\b\d+(?:\.\d+)?(?:e[+-]?\d+)?\b)|([A-Za-z_]\w*)(?=\s*\()|([A-Za-z_]\w*)/g;
  let last = 0;
  let m: RegExpExecArray | null;
  let i = 0;
  while ((m = re.exec(line))) {
    if (m.index > last) out.push(line.slice(last, m.index));
    const [text, com, str, num, fn, word] = m;
    const cls = com ? "tok-com" : str ? "tok-str" : num ? "tok-num" : fn ? (KEYWORDS.has(fn) ? "tok-kw" : "tok-fn") : word && KEYWORDS.has(word) ? "tok-kw" : null;
    out.push(cls ? <span key={i++} className={cls}>{text}</span> : text);
    last = m.index + text.length;
  }
  if (last < line.length) out.push(line.slice(last));
  return out;
}

function Code({ code, python }: { code: string; python: boolean }) {
  const lines = code.replace(/\r\n/g, "\n").split("\n");
  return (
    <div className="code">
      <pre>
        {lines.map((l, i) => (
          <div key={i}>
            <span className="ln">{i + 1}</span>
            {python ? highlight(l) : l}
          </div>
        ))}
      </pre>
    </div>
  );
}

export function ViewerView() {
  const app = useApp();
  const { viewerRequest, connected, settings } = app;
  const [doc, setDoc] = useState<DocView | null>(null);
  const [source, setSource] = useState<{ from: "local" | "handheld"; path: string } | null>(null);
  const [item, setItem] = useState<Item>({ kind: "summary" });
  const [loading, setLoading] = useState(false);

  async function load(from: "local" | "handheld", path: string) {
    setLoading(true);
    try {
      const d = from === "local" ? await api.openLocalDocument(path) : await api.openHandheldDocument(path);
      setDoc(d);
      setSource({ from, path });
      const first = d.pages[0];
      setItem(first ? { kind: "page", problem: first.problem, index: first.index, apps: first.apps } : { kind: "summary" });
    } catch (e) {
      app.ask({
        title: "Can't open the document",
        tone: "warn",
        text: (
          <p>
            <b>{baseName(path)}</b> isn't a readable TI-Nspire document: {errorText(e)}
          </p>
        ),
        choices: [{ id: "ok", label: "OK", primary: true }],
        cancel: "ok",
      });
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (viewerRequest) load(viewerRequest.source, viewerRequest.path);
  }, [viewerRequest]); // eslint-disable-line react-hooks/exhaustive-deps

  async function openFile() {
    const path = await open({ title: "Open a TI-Nspire document", filters: [{ name: "TI-Nspire documents", extensions: ["tns"] }], defaultPath: settings.localFolder ?? undefined });
    if (typeof path === "string") load("local", path);
  }

  async function savePython() {
    if (!doc) return;
    const folder = await open({ directory: true, title: "Save the Python files into…", defaultPath: settings.localFolder ?? undefined });
    if (typeof folder !== "string") return;
    try {
      const saved = await api.saveCodeFiles(folder, doc.python.map((p) => [p.name, p.code]));
      app.toast("ok", `Saved ${saved.length} Python file${saved.length === 1 ? "" : "s"}`, folder);
    } catch (e) {
      app.toast("bad", "Couldn't save", errorText(e));
    }
  }

  async function saveLua() {
    if (!doc?.lua) return;
    const path = await save({ title: "Save the Lua script", defaultPath: `${doc.name.replace(/\.tns$/i, "")}.lua`, filters: [{ name: "Lua", extensions: ["lua"] }] });
    if (!path) return;
    await api.writeTextFile(path, doc.lua).then(() => app.toast("ok", "Saved", path), (e) => app.toast("bad", "Couldn't save", errorText(e)));
  }

  async function sendToHandheld() {
    if (!source || source.from !== "local") return;
    const folder = settings.remoteFolder || "/";
    const there = await api.exists([`${folder.replace(/\/$/, "")}/${baseName(source.path)}`]).catch(() => [false]);
    if (there[0] && !(await app.confirm(`Replace “${baseName(source.path)}”?`, <p>It's already in {folder} on the handheld.</p>, "Replace", true))) return;
    api.upload([source.path], folder, "replace").catch((e) => app.toast("bad", "Couldn't send", errorText(e)));
  }

  async function receiveCopy() {
    if (!source || source.from !== "handheld") return;
    const folder = await open({ directory: true, title: "Save a copy into…", defaultPath: settings.localFolder ?? undefined });
    if (typeof folder === "string") api.download([source.path], folder).catch((e) => app.toast("bad", "Couldn't receive", errorText(e)));
  }

  const notesFor = useMemo(() => {
    const m = new Map<string, DocView["notes"]>();
    for (const n of doc?.notes ?? []) {
      const k = `${n.problem}.${n.page}`;
      m.set(k, [...(m.get(k) ?? []), n]);
    }
    return m;
  }, [doc]);

  function body(): ReactNode {
    if (!doc) {
      return (
        <div className="pane-empty">
          <IconBook />
          <p>Open a .tns file from this computer, or double-click a document in the handheld's list.</p>
          <button className="btn primary" onClick={openFile}>
            <IconOpen /> Open .tns…
          </button>
        </div>
      );
    }
    switch (item.kind) {
      case "summary":
        return <pre className="summary">{doc.summary}</pre>;
      case "python": {
        const f = doc.python.find((p) => p.name === item.name);
        return f ? <Code code={f.code} python /> : null;
      }
      case "lua":
        return doc.lua ? <Code code={doc.lua} python={false} /> : null;
      case "page": {
        const notes = notesFor.get(`${item.problem}.${item.index}`);
        if (notes?.length) {
          return notes.map((n, i) =>
            n.error ? (
              <div key={i} className="callout bad">
                This page couldn't be drawn ({n.error}).
              </div>
            ) : (
              <div key={i} className="handheld-page big">
                <Paragraphs paragraphs={n.paragraphs} />
              </div>
            ),
          );
        }
        if (item.apps.some((a) => a.startsWith("Python")) && doc.python.length) {
          return doc.python.map((p) => (
            <div key={p.name} style={{ width: "100%", maxWidth: 820 }}>
              <div className="page-label">{p.name}</div>
              <Code code={p.code} python />
            </div>
          ));
        }
        if (item.apps.includes("Lua script") && doc.lua) return <Code code={doc.lua} python={false} />;
        return (
          <div className="callout info" style={{ maxWidth: 560 }}>
            {item.apps.length ? `${item.apps.join(", ")} pages need TI's software to display.` : "This page is empty."} CalcBridge can still transfer, back up and organise
            this document.
          </div>
        );
      }
    }
  }

  return (
    <div className="view">
      <div className="view-head">
        <h1>Viewer</h1>
        {doc && (
          <span className="sub">
            <b style={{ color: "var(--text)" }}>{doc.name}</b> · from {source?.from === "handheld" ? "the handheld" : "this computer"} · {formatSize(doc.size)}
          </span>
        )}
        {loading && <span className="spinner" />}
        <div className="grow" />
        <div className="toolbar">
          <button className="btn" onClick={openFile}>
            <IconOpen /> Open .tns…
          </button>
          {doc && doc.python.length > 0 && (
            <>
              <button className="btn" onClick={savePython}>
                <IconSave /> Save Python files…
              </button>
              <button className="btn" onClick={() => app.openInCreate("python", doc.python[0].name.replace(/\.py$/, ""), doc.python[0].code)}>
                <IconCreate /> Edit in Create
              </button>
            </>
          )}
          {doc?.lua && (
            <button className="btn" onClick={saveLua}>
              <IconSave /> Save Lua script…
            </button>
          )}
          {source?.from === "local" && (
            <button className="btn primary" onClick={sendToHandheld} disabled={!connected}>
              <IconSend /> Send to handheld
            </button>
          )}
          {source?.from === "handheld" && (
            <button className="btn" onClick={receiveCopy}>
              <IconSave /> Save a copy…
            </button>
          )}
        </div>
      </div>
      <div className="viewer-layout">
        <div className="outline">
          {doc && (
            <>
              <div className="group">Pages</div>
              {doc.pages.map((p) => (
                <button
                  key={`${p.problem}.${p.index}`}
                  className={item.kind === "page" && item.problem === p.problem && item.index === p.index ? "on" : ""}
                  onClick={() => setItem({ kind: "page", problem: p.problem, index: p.index, apps: p.apps })}
                  title={p.apps.join(", ")}
                >
                  {p.problem}.{p.index} {p.apps.length ? p.apps.join(", ") : "empty"}
                </button>
              ))}
              {doc.python.length > 0 && <div className="group">Python files</div>}
              {doc.python.map((p) => (
                <button key={p.name} className={item.kind === "python" && item.name === p.name ? "on" : ""} onClick={() => setItem({ kind: "python", name: p.name })}>
                  {p.name}
                </button>
              ))}
              {doc.lua && (
                <>
                  <div className="group">Lua</div>
                  <button className={item.kind === "lua" ? "on" : ""} onClick={() => setItem({ kind: "lua" })}>
                    Script
                  </button>
                </>
              )}
              <div className="group">Document</div>
              <button className={item.kind === "summary" ? "on" : ""} onClick={() => setItem({ kind: "summary" })}>
                What's inside
              </button>
            </>
          )}
        </div>
        <div className="doc-body">{body()}</div>
      </div>
    </div>
  );
}
