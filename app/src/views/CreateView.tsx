// Create: write a Notes page with real 2D math, a Python program, a Lua script or plain text, see
// it as the handheld will, and save it as .tns or send it straight to the handheld.
import { useEffect, useMemo, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, baseName, errorKind, errorText, type DocKind, type DocSpec, type PreviewPage } from "../api";
import { Formula, Paragraphs } from "../components/Math";
import { IconOpen, IconSave, IconSend } from "../icons";
import { useApp } from "../store";

const KINDS: { kind: DocKind; label: string; hint: string }[] = [
  { kind: "notes", label: "Notes with math", hint: "A Notes page with real 2D math boxes" },
  { kind: "python", label: "Python", hint: "A program for the handheld's Python editor" },
  { kind: "lua", label: "Lua", hint: "A Lua script document" },
  { kind: "text", label: "Text", hint: "Plain text on a Notes page" },
];

const STARTERS: Record<DocKind, string> = {
  notes: [
    "# Newton's method",
    "To find a root of $f$, start at $x_{0}$ and repeat",
    "$x_{k+1}=x_{k}-\\frac{f(x_{k})}{f'(x_{k})}$",
    "It converges quickly near a simple root: the error roughly squares each step, $\\abs{e_{k+1}}\\approx C e_{k}^{2}$.",
    "---",
    "# Example",
    "**Find** $\\sqrt{2}$ with $f(x)=x^{2}-2$, starting at $x_{0}=1$.",
  ].join("\n"),
  python: [
    "# Newton's method",
    "def newton(f, df, x, steps=6):",
    "    for k in range(steps):",
    "        x = x - f(x) / df(x)",
    "        print(k + 1, x)",
    "    return x",
    "",
    "newton(lambda x: x*x - 2,",
    "       lambda x: 2*x, 1.0)",
  ].join("\n"),
  lua: ['-- draws a greeting', "function on.paint(gc)", '  gc:drawString("Hello from CalcBridge", 10, 20)', "end"].join("\n"),
  text: "Write anything here.\nEach line becomes a line on the handheld.",
};

const SYNTAX: [string, string | null][] = [
  ["# Title", null],
  ["$x^{2}$", "x^{2}"],
  ["$x_{k+1}$", "x_{k+1}"],
  ["$\\frac{a}{b}$", "\\frac{a}{b}"],
  ["$\\sqrt{x}$", "\\sqrt{x}"],
  ["$\\abs{x}$", "\\left|x\\right|"],
  ["$\\hat{y}$", "y^{\\wedge}"],
  ["$\\alpha \\pi \\le \\infty$", "\\alpha\\,\\pi\\le\\infty"],
  ["**bold**", null],
  ["---", null],
];
const SYNTAX_NOTES: Record<string, string> = {
  "# Title": "a bold, coloured title line",
  "**bold**": "bold text",
  "---": "a line on its own starts a new page",
};

const TEMPLATES: [label: string, snippet: string][] = [
  ["a⁄b", String.raw`\frac{}{}`],
  ["√x", String.raw`\sqrt{}`],
  ["xⁿ", "^{}"],
  ["xₖ", "_{}"],
  ["|x|", String.raw`\abs{}`],
  ["x̂", String.raw`\hat{}`],
];
const SYMBOLS = "ε π θ τ λ Δ Σ ≤ ≥ ≠ ≈ ± → ∞ … · ×".split(" ");

/** Is the cursor inside a $…$ math section of its line? */
function insideMath(text: string, at: number): boolean {
  const lineStart = text.lastIndexOf("\n", at - 1) + 1;
  return (text.slice(lineStart, at).match(/\$/g)?.length ?? 0) % 2 === 1;
}

const DRAFT = "calcbridge.createDraft";

interface Draft {
  kind: DocKind;
  name: string;
  content: Record<DocKind, string>;
}

function loadDraft(): Draft {
  try {
    const d = JSON.parse(localStorage.getItem(DRAFT) ?? "null");
    if (d && d.kind && d.content) return { ...d, content: { ...STARTERS, ...d.content } };
  } catch {
    /* start fresh */
  }
  return { kind: "notes", name: "My notes", content: { ...STARTERS } };
}

export function CreateView() {
  const app = useApp();
  const { connected, settings, updateSettings, createRequest } = app;
  const [draft, setDraft] = useState<Draft>(loadDraft);
  const [pages, setPages] = useState<PreviewPage[]>([]);
  const [busy, setBusy] = useState(false);
  const [handheldWidth, setHandheldWidth] = useState(true);
  const editor = useRef<HTMLTextAreaElement>(null);
  const content = draft.content[draft.kind];

  useEffect(() => {
    localStorage.setItem(DRAFT, JSON.stringify(draft));
  }, [draft]);

  /** Put new text in the editor; if what's there for that kind was edited, ask first. */
  async function load(kind: DocKind, name: string, text: string) {
    const current = draft.content[kind];
    const edited = current.trim() !== "" && current !== STARTERS[kind] && current !== text;
    if (edited && !(await app.confirm("Replace what's in the editor?", <p>The {KINDS.find((k) => k.kind === kind)?.label} text you were writing will be replaced by {name}.</p>, "Replace"))) {
      return;
    }
    setDraft((d) => ({ ...d, kind, name, content: { ...d.content, [kind]: text } }));
  }

  // "Open in Create" from the Files tab or the Viewer
  useEffect(() => {
    if (createRequest) load(createRequest.kind, createRequest.name, createRequest.content);
  }, [createRequest]); // eslint-disable-line react-hooks/exhaustive-deps

  // live preview of Notes (debounced)
  useEffect(() => {
    if (draft.kind !== "notes" && draft.kind !== "text") return;
    const source = draft.kind === "text" ? content.replace(/\$/g, "＄") : content; // text is taken literally
    const t = window.setTimeout(() => {
      api.previewNotes(draft.kind === "text" ? escapeText(content) : source).then(setPages).catch(() => {});
    }, 120);
    return () => window.clearTimeout(t);
  }, [content, draft.kind]); // eslint-disable-line react-hooks/exhaustive-deps

  const setContent = (v: string) => setDraft((d) => ({ ...d, content: { ...d.content, [d.kind]: v } }));
  const spec = (): DocSpec => ({ kind: draft.kind, name: draft.name.trim() || "Untitled", content });
  const errors = pages.flatMap((p) => p.errors);
  const longLines = useMemo(
    () => (draft.kind === "python" ? content.split("\n").filter((l) => l.length > 42).length : 0),
    [content, draft.kind],
  );

  async function saveFile() {
    const path = await save({
      title: "Save the document",
      defaultPath: `${settings.localFolder ? settings.localFolder.replace(/[\\/]+$/, "") + "/" : ""}${spec().name}.tns`,
      filters: [{ name: "TI-Nspire document", extensions: ["tns"] }],
    });
    if (!path) return;
    try {
      await api.saveDocument(spec(), path);
      app.toast("ok", "Saved", path);
    } catch (e) {
      app.toast("bad", "Couldn't save", errorText(e));
    }
  }

  async function send(replace = false) {
    const folder = settings.createFolder || "/";
    setBusy(true);
    try {
      const path = await api.sendDocument(spec(), folder, replace);
      app.toast("ok", `Sent ${baseName(path)}`, `It's in ${folder === "/" ? "My Documents" : folder} on the handheld.`);
      app.bumpHandheld();
    } catch (e) {
      if (errorKind(e) === "exists" && !replace) {
        const ok = await app.confirm(`Replace “${spec().name}.tns”?`, <p>There's already a document with that name in {folder} on the handheld.</p>, "Replace", true);
        if (ok) return send(true);
      } else app.toast("bad", "Couldn't send", errorText(e));
    } finally {
      setBusy(false);
    }
  }

  async function openFile() {
    const path = await open({
      title: "Open a file to turn into a document",
      filters: [{ name: "Python, Lua, text or Notes markup", extensions: ["py", "lua", "txt", "md", "notes"] }],
    });
    if (typeof path !== "string") return;
    try {
      const text = await api.readTextFile(path);
      const lower = path.toLowerCase();
      const kind: DocKind = lower.endsWith(".py") ? "python" : lower.endsWith(".lua") ? "lua" : lower.endsWith(".txt") ? "text" : "notes";
      await load(kind, baseName(path).replace(/\.[^.]+$/, ""), text);
    } catch (e) {
      app.toast("bad", "Couldn't open it", errorText(e));
    }
  }

  /** Insert at the cursor (replacing a selection). Math snippets outside $…$ get wrapped in it, and
   * the cursor lands inside the first {}. */
  function insert(snippet: string, math = false) {
    const el = editor.current;
    if (!el) return;
    const { selectionStart: a, selectionEnd: b, value } = el;
    const text = math && !insideMath(value, a) ? `$${snippet}$` : snippet;
    const next = value.slice(0, a) + text + value.slice(b);
    setContent(next);
    const hole = text.indexOf("{}");
    const caret = a + (hole >= 0 ? hole + 1 : text.length - (text !== snippet ? 1 : 0));
    requestAnimationFrame(() => {
      el.focus();
      el.setSelectionRange(caret, caret);
    });
  }

  async function installModule(replace = false): Promise<void> {
    setBusy(true);
    try {
      const path = await api.installModule(spec(), replace);
      app.toast("ok", `Installed ${baseName(path)} in PyLib`, "On the handheld, choose Refresh Libraries (or just import it from another program).");
      app.bumpHandheld();
    } catch (e) {
      if (errorKind(e) === "exists" && !replace) {
        const ok = await app.confirm("Replace the module?", <p>PyLib already has a module with this name.</p>, "Replace", true);
        if (ok) return installModule(true);
      } else app.toast("bad", "Couldn't install the module", errorText(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="view">
      <div className="view-head">
        <h1>Create</h1>
        <div className="seg" role="tablist">
          {KINDS.map((k) => (
            <button key={k.kind} className={draft.kind === k.kind ? "on" : ""} onClick={() => setDraft((d) => ({ ...d, kind: k.kind }))} title={k.hint}>
              {k.label}
            </button>
          ))}
        </div>
        <div className="grow" />
        <button className="btn ghost" onClick={openFile}>
          <IconOpen /> Open file…
        </button>
      </div>
      <div className="doc-form">
        <label>Name</label>
        <input value={draft.name} onChange={(e) => setDraft((d) => ({ ...d, name: e.target.value }))} spellCheck={false} />
        <span className="dim">.tns</span>
        <div style={{ flex: 1 }} />
        <button className="btn" onClick={saveFile} disabled={errors.length > 0}>
          <IconSave /> Save .tns…
        </button>
        <label>Handheld folder</label>
        <input style={{ width: 130 }} value={settings.createFolder} onChange={(e) => updateSettings({ createFolder: e.target.value.startsWith("/") ? e.target.value : `/${e.target.value}` })} spellCheck={false} />
        <button className="btn primary" onClick={() => send()} disabled={!connected || busy || errors.length > 0} title={connected ? "" : "Connect a calculator first"}>
          <IconSend /> Send to handheld
        </button>
        {draft.kind === "python" && (
          <button
            className="btn"
            onClick={() => installModule()}
            disabled={!connected || busy}
            title="Put it in /PyLib under its module name, so other programs can import it"
          >
            Install as module
          </button>
        )}
      </div>
      <div className="create-layout">
        <div className="editor">
          <div className="bar">
            {draft.kind === "notes" ? (
              <div style={{ display: "flex", flexWrap: "wrap", gap: 2, alignItems: "center" }}>
                {TEMPLATES.map(([label, snip]) => (
                  <button key={label} className="btn small ghost" style={{ padding: "0 7px" }} onClick={() => insert(snip, true)} title={snip}>
                    {label}
                  </button>
                ))}
                <span className="sep" style={{ width: 1, height: 16, background: "var(--border)", margin: "0 4px" }} />
                {SYMBOLS.map((sym) => (
                  <button key={sym} className="btn small ghost" style={{ padding: "0 6px", minWidth: 24 }} onClick={() => insert(sym, true)}>
                    {sym}
                  </button>
                ))}
                <span className="sep" style={{ width: 1, height: 16, background: "var(--border)", margin: "0 4px" }} />
                <button className="btn small ghost" onClick={() => insert("\n---\n")} title="Start a new page">
                  new page
                </button>
              </div>
            ) : draft.kind === "python" ? (
              <span>{longLines > 0 ? `${longLines} line${longLines === 1 ? " is" : "s are"} longer than 42 characters and will scroll on the handheld.` : "The handheld's editor shows about 42 characters per line."}</span>
            ) : (
              <span>{KINDS.find((k) => k.kind === draft.kind)?.hint}</span>
            )}
          </div>
          <textarea
            ref={editor}
            value={content}
            spellCheck={false}
            className={draft.kind === "notes" || draft.kind === "text" ? "wrap" : ""}
            onChange={(e) => setContent(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Tab" && !e.shiftKey) {
                e.preventDefault();
                insert(draft.kind === "python" ? "    " : "\t");
              }
            }}
          />
        </div>
        <div style={{ display: "flex", flexDirection: "column", gap: 10, minHeight: 0 }}>
          {draft.kind === "notes" || draft.kind === "text" ? (
            <div className="preview">
              {errors.length > 0 && (
                <div className="errors" style={{ alignSelf: "stretch" }}>
                  {errors.map(([line, msg]) => (
                    <div key={line}>
                      Line {line}: {msg}
                    </div>
                  ))}
                </div>
              )}
              {pages.map((p, i) => (
                <div key={i} style={{ display: "contents" }}>
                  {pages.length > 1 && <div className="page-label">Page {i + 1}</div>}
                  <div className={`handheld-page${handheldWidth ? "" : " big"}`}>
                    <Paragraphs paragraphs={p.paragraphs} />
                  </div>
                </div>
              ))}
            </div>
          ) : (
            <div className="preview">
              <div className="callout info" style={{ alignSelf: "stretch" }}>
                {draft.kind === "python"
                  ? "The document opens in the handheld's Python editor; run it with ctrl+R. Below: roughly what fits on its screen."
                  : "Lua scripts run as a page of their own. Use on.paint and friends from TI's Lua API."}
              </div>
              {draft.kind === "python" && (
                <div className="handheld-page py-preview">
                  {content.split("\n").map((line, i) => (
                    <div key={i} className={line.length > 42 ? "long" : ""}>
                      {line.slice(0, 42) || " "}
                      {line.length > 42 && <span className="more">…</span>}
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
          {(draft.kind === "notes" || draft.kind === "text") && (
            <label className="check" style={{ fontSize: 12.5, color: "var(--text-2)" }}>
              <input type="checkbox" checked={handheldWidth} onChange={(e) => setHandheldWidth(e.target.checked)} />
              Preview at the handheld's width (320 px); its fonts differ a little
            </label>
          )}
          {draft.kind === "notes" && (
            <div className="card" style={{ padding: 12, gap: 8 }}>
              <div className="syntax">
                {SYNTAX.map(([code, latex]) => (
                  <div key={code} style={{ display: "contents" }}>
                    <code>{code}</code>
                    <span>{latex ? <Formula latex={latex} /> : SYNTAX_NOTES[code]}</span>
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

/** Plain text: nothing is markup. */
function escapeText(text: string): string {
  return text.replace(/\$/g, "＄").replace(/\*\*/g, "*​*");
}
