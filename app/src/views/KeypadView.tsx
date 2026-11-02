// Remote keypad: press the handheld's keys from the computer while watching its screen.
import { useEffect, useRef, useState } from "react";
import { api, errorText } from "../api";
import { LiveScreen, type LiveScreenHandle } from "../components/LiveScreen";
import { useFitScale } from "../components/useFit";
import { useApp } from "../store";

type K = [label: string, key: string, cls?: string];

const TOP: K[][] = [
  [["esc", "esc", "dark"], ["tab", "tab", "dark"], ["home", "home", "dark"], ["menu", "menu", "dark"]],
  [["ctrl", "ctrl", "accent"], ["shift", "shift", "dark"], ["var", "var", "dark"], ["del", "del", "dark"]],
];
const MATH: K[][] = [
  [["=", "="], ["sin", "sin"], ["cos", "cos"], ["tan", "tan"], ["^", "^"], ["x²", "square"]],
  [["10ˣ", "10^"], ["eˣ", "e^"], ["(", "("], [")", ")"], ["÷", "/"], ["×", "*"]],
  [["7", "7"], ["8", "8"], ["9", "9"], ["−", "-"], ["π", "pi"], ["?!", "?"]],
  [["4", "4"], ["5", "5"], ["6", "6"], ["+", "+"], ["EE", "EE"], [",", ","]],
  [["1", "1"], ["2", "2"], ["3", "3"], ["(−)", "neg"], ["θ", "theta"], ["flag", "flag"]],
  [["0", "0"], [".", "."], ["i", "imag"], ["catalog", "catalog"], [":", ":"], ["enter", "enter", "dark"]],
];
const LETTERS = ["abcdefg", "hijklmn", "opqrstu", "vwxyz"];

/** Computer keys that map onto handheld keys. */
const PC: Record<string, string> = {
  ArrowUp: "up", ArrowDown: "down", ArrowLeft: "left", ArrowRight: "right",
  Enter: "enter", Escape: "esc", Backspace: "del", Delete: "del", Tab: "tab", Home: "home",
};
const TYPABLE = new Set("0123456789=+-*/^(),.?:'\" <>".split(""));

export function KeypadView() {
  const app = useApp();
  const { connected, settings } = app;
  const screen = useRef<LiveScreenHandle>(null);
  const [capture, setCapture] = useState(false);
  const [text, setText] = useState("");
  const [flash, setFlash] = useState<string | null>(null);
  const pending = useRef(Promise.resolve());
  const [fitBox, scale] = useFitScale<HTMLDivElement>(320, 240, 40, 3);

  // one key at a time, in order; the screen catches up after each
  function press(key: string) {
    if (!connected) return;
    setFlash(key);
    window.setTimeout(() => setFlash((f) => (f === key ? null : f)), 160);
    pending.current = pending.current
      .then(() => api.pressKey(key))
      .then(() => {
        window.setTimeout(() => screen.current?.refresh(), 120);
      })
      .catch((e) => app.toast("bad", "The key didn't get through", errorText(e)));
  }

  useEffect(() => {
    if (!capture) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement) return;
      let key: string | undefined;
      if (e.key === "Tab" && e.shiftKey) key = "shift_tab";
      else if (e.key === "Enter" && e.ctrlKey) key = "ctrl_enter";
      else if (PC[e.key]) key = PC[e.key];
      else if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        if (/[a-zA-Z]/.test(e.key) || TYPABLE.has(e.key)) key = e.key;
      }
      if (key) {
        e.preventDefault();
        e.stopPropagation();
        press(key);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [capture, connected]); // eslint-disable-line react-hooks/exhaustive-deps

  async function sendText() {
    const t = text;
    if (!t) return;
    setText("");
    try {
      await api.typeText(t);
      window.setTimeout(() => screen.current?.refresh(), 150);
    } catch (e) {
      app.toast("bad", "Couldn't type that", errorText(e));
      setText(t);
    }
  }

  const key = ([label, k, cls]: K) => (
    <button key={k} className={`key ${cls ?? ""}${flash === k ? " flash" : ""}`} onClick={() => press(k)} disabled={!connected} title={k}>
      {label}
    </button>
  );

  return (
    <div className="view">
      <div className="view-head">
        <h1>Remote keypad</h1>
        <span className="sub">Keys act on the handheld immediately.</span>
      </div>
      <div className="keypad-layout">
        <div style={{ display: "flex", flexDirection: "column", gap: 12, minHeight: 0, minWidth: 0 }}>
          <div className="screen-wrap" ref={fitBox}>
            <LiveScreen ref={screen} fps={settings.liveFps} scale={scale} smooth={settings.smoothScreen || scale % 1 !== 0} />
          </div>
          <div className={`capture${capture ? " on" : ""}`} onClick={() => setCapture((c) => !c)} role="switch" aria-checked={capture}>
            {capture
              ? "Your keyboard is driving the handheld: arrows, Enter, Esc, Backspace, Tab, letters and digits. Click to stop."
              : "Click here to use your computer's keyboard as the handheld's keypad."}
          </div>
        </div>
        <div className="keypad">
          {TOP.map((row, i) => (
            <div className="row" key={i} style={{ gridTemplateColumns: "repeat(4, 1fr)" }}>
              {row.map(key)}
            </div>
          ))}
          <div className="touchpad">
            <span />
            {key(["▲", "up"])}
            <span />
            {key(["◀", "left"])}
            {key(["●", "click", "accent"])}
            {key(["▶", "right"])}
            <span />
            {key(["▼", "down"])}
            <span />
          </div>
          {MATH.map((row, i) => (
            <div className="row" key={i} style={{ gridTemplateColumns: "repeat(6, 1fr)" }}>
              {row.map(key)}
            </div>
          ))}
          <h3>Letters</h3>
          {LETTERS.map((row) => (
            <div className="row" key={row} style={{ gridTemplateColumns: "repeat(7, 1fr)" }}>
              {row.split("").map((c) => key([c.toUpperCase(), c, "letter"]))}
            </div>
          ))}
          <div className="row" style={{ gridTemplateColumns: "1fr 1fr" }}>
            {key(["space", " "])}
            {key(["shift+tab", "shift_tab"])}
          </div>
          <h3>Type text</h3>
          <div style={{ display: "flex", gap: 6 }}>
            <input
              style={{ flex: 1 }}
              placeholder="e.g. 2+2 then Send"
              value={text}
              disabled={!connected}
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && sendText()}
            />
            <button className="btn primary" onClick={sendText} disabled={!connected || !text}>
              Send
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
