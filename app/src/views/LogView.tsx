import { useEffect, useRef, useState } from "react";
import { api, on, type LogLine } from "../api";
import { IconCopy } from "../icons";
import { useApp } from "../store";

export function LogView() {
  const app = useApp();
  const [lines, setLines] = useState<LogLine[]>([]);
  const [debug, setDebug] = useState(false);
  const [clearedAt, setClearedAt] = useState(0);
  const box = useRef<HTMLDivElement>(null);
  const stick = useRef(true);

  useEffect(() => {
    api.logLines().then(setLines);
    const un = on.log((l) => setLines((all) => [...all.slice(-1999), l]));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const shown = lines.filter((l) => l.time >= clearedAt && (debug || l.level !== "debug"));

  useEffect(() => {
    const el = box.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [shown.length]);

  const text = () => shown.map((l) => `${new Date(l.time).toLocaleTimeString()}  ${l.level.toUpperCase().padEnd(5)}  ${l.message}`).join("\n");

  return (
    <div className="view">
      <div className="view-head">
        <h1>Log</h1>
        <span className="sub">What CalcBridge did, and any problems.</span>
        <div className="grow" />
        <label className="check">
          <input type="checkbox" checked={debug} onChange={(e) => setDebug(e.target.checked)} /> Details
        </label>
        <button
          className="btn"
          onClick={() =>
            navigator.clipboard.writeText(text()).then(
              () => app.toast("ok", "Log copied"),
              () => app.toast("bad", "Couldn't copy the log"),
            )
          }
        >
          <IconCopy /> Copy
        </button>
        <button className="btn ghost" onClick={() => setClearedAt(Date.now())}>
          Clear
        </button>
      </div>
      <div
        className="log"
        ref={box}
        onScroll={(e) => {
          const el = e.currentTarget;
          stick.current = el.scrollTop + el.clientHeight >= el.scrollHeight - 20;
        }}
      >
        {shown.length === 0 && <div className="debug">Nothing yet.</div>}
        {shown.map((l, i) => (
          <div key={i} className={l.level}>
            <span className="t">{new Date(l.time).toLocaleTimeString()}</span>
            {l.message}
          </div>
        ))}
      </div>
    </div>
  );
}
