// Screen: the handheld's display live, plus captures you can save, copy and collect.
import { useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import { api, errorText, joinLocal } from "../api";
import { LiveScreen, type LiveScreenHandle } from "../components/LiveScreen";
import { useFitScale } from "../components/useFit";
import { IconCamera, IconCopy, IconPause, IconPlay, IconRefresh, IconSave, IconTrash, IconX } from "../icons";
import { useApp } from "../store";

const RATES = [1, 2, 5, 10, 30];
const rateLabel = (r: number) => (r === 30 ? "Max" : `${r}/s`);

interface Capture {
  id: number;
  name: string;
  /** The raw frame, 1:1. */
  canvas: HTMLCanvasElement;
  thumb: string;
}

/** The frame as it should be saved or copied: TI's grey 3 px border if wanted, then scaled up. */
function render(src: HTMLCanvasElement, scale: number, border: boolean): HTMLCanvasElement {
  const b = border ? 3 : 0;
  const c = document.createElement("canvas");
  c.width = Math.round((src.width + 2 * b) * scale);
  c.height = Math.round((src.height + 2 * b) * scale);
  const g = c.getContext("2d")!;
  g.imageSmoothingEnabled = false;
  if (b) {
    g.fillStyle = "rgb(60, 60, 60)";
    g.fillRect(0, 0, c.width, c.height);
  }
  g.drawImage(src, b * scale, b * scale, src.width * scale, src.height * scale);
  return c;
}

function snapshot(canvas: HTMLCanvasElement): HTMLCanvasElement {
  const c = document.createElement("canvas");
  c.width = canvas.width;
  c.height = canvas.height;
  c.getContext("2d")!.drawImage(canvas, 0, 0);
  return c;
}

function today(): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getMonth() + 1)}-${p(d.getDate())}-${d.getFullYear()}`;
}

// Captures outlive the Screen view (it's unmounted when you switch tabs) for the whole session.
let keptCaptures: Capture[] = [];
let nextCapture = 1;

/** "name.png", or "name (2).png" and so on if that's taken in the folder. */
async function freeName(folder: string, name: string, sep: string): Promise<string> {
  for (let n = 1; ; n++) {
    const candidate = joinLocal(folder, n === 1 ? `${name}.png` : `${name} (${n}).png`, sep);
    const [taken] = await api.localExists([candidate]);
    if (!taken) return candidate;
  }
}

export async function copyImage(canvas: HTMLCanvasElement, toast: ReturnType<typeof useApp>["toast"]) {
  try {
    const blob = await new Promise<Blob | null>((r) => canvas.toBlob(r, "image/png"));
    if (!blob) throw new Error("no image");
    await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
    toast("ok", "Copied", "Paste it into a document or a chat.");
  } catch (e) {
    toast("bad", "Couldn't copy the image", errorText(e));
  }
}

async function writeImage(canvas: HTMLCanvasElement, path: string) {
  const jpeg = /\.jpe?g$/i.test(path);
  const url = canvas.toDataURL(jpeg ? "image/jpeg" : "image/png", 0.95);
  await api.savePng(path, url.slice(url.indexOf(",") + 1));
}

export function ScreenView() {
  const app = useApp();
  const { settings, updateSettings, connected } = app;
  const screen = useRef<LiveScreenHandle>(null);
  const [playing, setPlaying] = useState(true);
  const [fps, setFps] = useState(0);
  const [zoom, setZoom] = useState(0);
  const [border, setBorder] = useState(false);
  const [captures, setCapturesState] = useState<Capture[]>(keptCaptures);
  const setCaptures = (update: (all: Capture[]) => Capture[]) =>
    setCapturesState((all) => (keptCaptures = update(all)));
  const [box, fit] = useFitScale<HTMLDivElement>(320, 240, 44, 4);
  const scale = zoom === 0 ? fit : zoom;
  const outScale = zoom === 0 ? 1 : zoom;

  const frame = () => screen.current?.canvas() ?? null;

  function capture() {
    const c = frame();
    if (!c || !connected) return;
    const raw = snapshot(c);
    const name = `${today()} Image ${String(nextCapture++).padStart(3, "0")}`;
    setCaptures((all) => [...all, { id: Date.now(), name, canvas: raw, thumb: raw.toDataURL("image/png") }]);
  }

  async function saveOne(c: HTMLCanvasElement, name: string) {
    const path = await save({
      title: "Save the screenshot",
      defaultPath: `${settings.localFolder ? settings.localFolder.replace(/[\\/]+$/, "") + "/" : ""}${name}.png`,
      filters: [
        { name: "PNG image", extensions: ["png"] },
        { name: "JPEG image", extensions: ["jpg", "jpeg"] },
      ],
    });
    if (!path) return;
    try {
      await writeImage(render(c, outScale, border), path);
      app.toast("ok", "Saved", path);
    } catch (e) {
      app.toast("bad", "Couldn't save the image", errorText(e));
    }
  }

  async function saveAll() {
    const folder = await open({ directory: true, title: "Save all captures into…", defaultPath: settings.localFolder ?? undefined });
    if (typeof folder !== "string") return;
    const places = await api.localPlaces();
    let n = 0;
    for (const cap of captures) {
      try {
        await writeImage(render(cap.canvas, outScale, border), await freeName(folder, cap.name, places.separator));
        n++;
      } catch (e) {
        app.toast("bad", `Couldn't save ${cap.name}`, errorText(e));
      }
    }
    app.toast("ok", `Saved ${n} capture${n === 1 ? "" : "s"}`, folder);
  }

  return (
    <div className="view">
      <div className="view-head">
        <h1>Screen</h1>
        <div className="grow" />
        <div className="toolbar">
          <button className="btn" onClick={() => setPlaying((p) => !p)} disabled={!connected} title={playing ? "Pause the live view" : "Show it live"}>
            {playing ? <IconPause /> : <IconPlay />} {playing ? "Pause" : "Live"}
          </button>
          <select value={settings.liveFps} onChange={(e) => updateSettings({ liveFps: Number(e.target.value) })} title="Refresh rate">
            {RATES.map((r) => (
              <option key={r} value={r}>
                {rateLabel(r)}
              </option>
            ))}
          </select>
          <span className="fps">{playing && connected && fps > 0 ? `${fps.toFixed(1)} fps` : ""}</span>
          <button className="btn icon" onClick={() => screen.current?.refresh()} disabled={!connected} title="Refresh now">
            <IconRefresh />
          </button>
          <div className="sep" />
          <div className="seg" role="group" aria-label="Zoom">
            {[1, 2, 3, 0].map((z) => (
              <button key={z} className={zoom === z ? "on" : ""} onClick={() => setZoom(z)} title={z ? `Show and save at ${z}×` : "Fill the window"}>
                {z === 0 ? "Fit" : `${z}×`}
              </button>
            ))}
          </div>
          <label className="check" title="Add TI's grey border to saved and copied images">
            <input type="checkbox" checked={border} onChange={(e) => setBorder(e.target.checked)} />
            Border
          </label>
          <div className="sep" />
          <button className="btn" onClick={() => frame() && copyImage(render(frame()!, outScale, border), app.toast)} disabled={!connected} title="Copy the screen as an image">
            <IconCopy /> Copy
          </button>
          <button className="btn" onClick={() => frame() && saveOne(snapshot(frame()!), `${today()} Screen`)} disabled={!connected}>
            <IconSave /> Save…
          </button>
          <button className="btn primary" onClick={capture} disabled={!connected} title="Keep this frame in the strip below">
            <IconCamera /> Capture
          </button>
        </div>
      </div>
      <div className="screen-wrap" ref={box}>
        <LiveScreen ref={screen} fps={playing ? settings.liveFps : 0} scale={scale} smooth={settings.smoothScreen || scale % 1 !== 0} onFps={setFps} />
      </div>
      {captures.length > 0 && (
        <div className="captures">
          <div className="captures-head">
            <b>Captures</b>
            <span className="dim">{captures.length}</span>
            <div style={{ flex: 1 }} />
            <button className="btn small" onClick={saveAll}>
              <IconSave /> Save all…
            </button>
            <button className="btn small ghost" onClick={() => setCaptures(() => [])}>
              <IconTrash /> Clear
            </button>
          </div>
          <div className="strip">
            {captures.map((cap) => (
              <figure key={cap.id} className="cap">
                <img src={cap.thumb} alt={cap.name} />
                <figcaption title={cap.name}>{cap.name}</figcaption>
                <div className="cap-actions">
                  <button className="btn small icon" title="Save…" onClick={() => saveOne(cap.canvas, cap.name)}>
                    <IconSave />
                  </button>
                  <button className="btn small icon" title="Copy" onClick={() => copyImage(render(cap.canvas, outScale, border), app.toast)}>
                    <IconCopy />
                  </button>
                  <button className="btn small icon" title="Remove" onClick={() => setCaptures((all) => all.filter((c) => c.id !== cap.id))}>
                    <IconX />
                  </button>
                </div>
              </figure>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
