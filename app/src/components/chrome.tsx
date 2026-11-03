// Window furniture: dialogs, toasts, the jobs bar and context menus.
import { useEffect, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { api } from "../api";
import { IconAlert, IconInfo, IconX } from "../icons";
import { useApp } from "../store";

export function DialogHost() {
  const { dialog } = useApp();
  const [value, setValue] = useState("");
  const [checked, setChecked] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!dialog) return;
    setValue(dialog.input?.value ?? "");
    setChecked(false);
    setError(null);
  }, [dialog]);

  useLayoutEffect(() => {
    const el = input.current;
    if (!dialog?.input || !el) return;
    el.focus();
    const v = dialog.input.value;
    const dot = v.lastIndexOf(".");
    if (dialog.input.selectStem && dot > 0) el.setSelectionRange(0, dot);
    else el.select();
  }, [dialog]);

  if (!dialog) return null;
  const finish = (choice: string) => {
    const chosen = dialog.choices.find((c) => c.id === choice);
    if (dialog.input && chosen && chosen.id !== dialog.cancel) {
      const problem = dialog.input.validate?.(value) ?? null;
      if (problem) {
        setError(problem);
        return;
      }
    }
    dialog.resolve({ choice, value, checked });
  };
  const primary = dialog.choices.find((c) => c.primary) ?? dialog.choices[dialog.choices.length - 1];
  const icon = dialog.tone === "danger" || dialog.tone === "warn" ? <IconAlert style={{ color: dialog.tone === "danger" ? "var(--danger)" : "var(--warn)" }} /> : dialog.tone === "info" ? <IconInfo /> : null;

  return (
    <div
      className="backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && dialog.cancel) finish(dialog.cancel);
      }}
    >
      <div
        className={`dialog${dialog.wide ? " wide" : ""}`}
        role="dialog"
        aria-modal="true"
        onKeyDown={(e) => {
          if (e.key === "Escape" && dialog.cancel) {
            e.stopPropagation();
            finish(dialog.cancel);
          } else if (e.key === "Enter" && !(e.target instanceof HTMLButtonElement) && !(e.target instanceof HTMLTextAreaElement)) {
            e.preventDefault();
            finish(primary.id);
          }
        }}
      >
        <h2>
          {icon}
          {dialog.title}
        </h2>
        {dialog.text && <div className="dialog-text">{typeof dialog.text === "string" ? <p>{dialog.text}</p> : dialog.text}</div>}
        {dialog.input && (
          <div className="field">
            {dialog.input.label && <label>{dialog.input.label}</label>}
            <input
              ref={input}
              value={value}
              spellCheck={false}
              onChange={(e) => {
                setValue(e.target.value);
                setError(null);
              }}
            />
            {error ? <div className="error">{error}</div> : dialog.input.hint && <div className="hint">{dialog.input.hint}</div>}
          </div>
        )}
        {dialog.checkbox && (
          <label className="check">
            <input type="checkbox" checked={checked} onChange={(e) => setChecked(e.target.checked)} />
            {dialog.checkbox}
          </label>
        )}
        <div className="buttons">
          {dialog.choices.map((c) => (
            <button
              key={c.id}
              className={`btn${c.primary ? " primary" : ""}${c.danger ? " danger solid" : ""}`}
              autoFocus={!dialog.input && c === primary}
              onClick={() => finish(c.id)}
            >
              {c.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}

export function Toasts() {
  const { toasts, dismissToast } = useApp();
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className={`toast ${t.kind}`}>
          <div className="body">
            <b>{t.title}</b>
            {t.text && <p>{t.text}</p>}
          </div>
          <button title="Dismiss" onClick={() => dismissToast(t.id)}>
            <IconX width={16} height={16} />
          </button>
        </div>
      ))}
    </div>
  );
}

export function JobsBar() {
  const { jobs, status } = useApp();
  const running = jobs.find((j) => j.state === "running");
  const queued = jobs.filter((j) => j.state === "queued").length;
  if (!running) {
    return (
      <div className="jobs">
        <div className="idle">
          {queued > 0
            ? `${queued} waiting…`
            : status.state === "connected"
              ? "Ready."
              : status.state === "connecting"
                ? status.detail
                : "Not connected."}
        </div>
      </div>
    );
  }
  const pct = running.total > 0 ? Math.min(100, (running.done / running.total) * 100) : 0;
  return (
    <div className="jobs">
      <div className="what">
        <div className="line">
          <b>{running.title}</b>
          <span>
            {running.total > 0 && `${Math.floor(pct)}%`} {running.detail}
            {queued > 0 && ` · ${queued} more waiting`}
          </span>
        </div>
        <div className={`progress${running.total > 0 ? "" : " indeterminate"}`}>
          <div style={{ width: `${pct}%` }} />
        </div>
      </div>
      <button className="btn small" onClick={() => api.cancelJob(running.id)}>
        Cancel
      </button>
    </div>
  );
}

// ------------------------------------------------------------- context menu
export interface MenuItem {
  label: string;
  icon?: ReactNode;
  kbd?: string;
  disabled?: boolean;
  danger?: boolean;
  action: () => void;
}

export type MenuEntry = MenuItem | "-";

export function useContextMenu() {
  const [menu, setMenu] = useState<{ x: number; y: number; items: MenuEntry[] } | null>(null);
  const open = (e: { clientX: number; clientY: number; preventDefault: () => void }, items: MenuEntry[]) => {
    e.preventDefault();
    setMenu({ x: e.clientX, y: e.clientY, items });
  };
  const element = menu ? <ContextMenu {...menu} onClose={() => setMenu(null)} /> : null;
  return { open, element };
}

function ContextMenu({ x, y, items, onClose }: { x: number; y: number; items: MenuEntry[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({ x: Math.min(x, window.innerWidth - r.width - 6), y: Math.min(y, window.innerHeight - r.height - 6) });
    el.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [x, y]);
  useEffect(() => {
    const close = (e: Event) => {
      if (ref.current && e.target instanceof Node && ref.current.contains(e.target)) return;
      onClose();
    };
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close, true);
    window.addEventListener("blur", onClose);
    window.addEventListener("keydown", key, true);
    return () => {
      window.removeEventListener("mousedown", close, true);
      window.removeEventListener("blur", onClose);
      window.removeEventListener("keydown", key, true);
    };
  }, [onClose]);
  return (
    <div className="menu" ref={ref} style={{ left: pos.x, top: pos.y }} role="menu">
      {items.map((item, i) =>
        item === "-" ? (
          <hr key={i} />
        ) : (
          <button
            key={i}
            role="menuitem"
            disabled={item.disabled}
            style={item.danger ? { color: "var(--danger)" } : undefined}
            onClick={() => {
              onClose();
              item.action();
            }}
          >
            {item.icon}
            {item.label}
            {item.kbd && <span className="kbd">{item.kbd}</span>}
          </button>
        ),
      )}
    </div>
  );
}
