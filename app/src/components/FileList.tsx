// A sortable, multi-select file table shared by both panes.
import { useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent, type PointerEvent, type ReactNode } from "react";
import { formatDate, formatSize } from "../api";
import { IconCode, IconDoc, IconFile, IconFolder } from "../icons";

export interface Row {
  key: string;
  name: string;
  isDir: boolean;
  size: number;
  /** Seconds since 1970, if known. */
  modified: number | null;
  kind: string;
}

type SortKey = "name" | "size" | "kind" | "modified";

export function kindOf(name: string, isDir: boolean): string {
  if (isDir) return "Folder";
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  switch (ext) {
    case "tns":
      return "TI-Nspire document";
    case "py":
      return "Python script";
    case "lua":
      return "Lua script";
    case "txt":
    case "md":
      return "Text";
    case "tco2":
    case "tcc2":
    case "tct2":
      return "TI-Nspire OS";
    case "png":
    case "jpg":
    case "jpeg":
    case "bmp":
      return "Image";
    case "csv":
      return "CSV";
    default:
      return ext ? `${ext.toUpperCase()} file` : "File";
  }
}

export function RowIcon({ row }: { row: Pick<Row, "name" | "isDir"> }) {
  if (row.isDir) return <IconFolder className="ic-folder" />;
  const n = row.name.toLowerCase();
  if (n.endsWith(".tns")) return <IconDoc className="ic-doc" />;
  if (n.endsWith(".py") || n.endsWith(".lua")) return <IconCode className="ic-py" />;
  return <IconFile className="ic-file" />;
}

interface Props {
  rows: Row[];
  selected: Set<string>;
  onSelect: (keys: Set<string>) => void;
  onOpen: (row: Row) => void;
  onContext: (e: MouseEvent, row: Row | null) => void;
  onKey?: (e: KeyboardEvent) => void;
  /** Start dragging the selection (pointer events; the native drag-and-drop is busy with Explorer). */
  onDragStart?: (rows: Row[], e: PointerEvent) => void;
  /** Highlight this folder row as a drop target. */
  dropKey?: string | null;
  showDate: boolean;
  empty?: ReactNode;
  filter: string;
}

export function FileList(props: Props) {
  const { rows, selected, onSelect, onOpen, onContext, onKey, onDragStart, dropKey, showDate, empty, filter } = props;
  const [sort, setSort] = useState<{ key: SortKey; asc: boolean }>({ key: "name", asc: true });
  const anchor = useRef<string | null>(null);
  const press = useRef<{ x: number; y: number; row: Row; started: boolean } | null>(null);
  const box = useRef<HTMLDivElement>(null);

  const shown = useMemo(() => {
    const f = filter.trim().toLowerCase();
    const list = f ? rows.filter((r) => r.name.toLowerCase().includes(f)) : rows.slice();
    const dir = sort.asc ? 1 : -1;
    list.sort((a, b) => {
      if (a.isDir !== b.isDir) return a.isDir ? -1 : 1; // folders first, either way
      let c = 0;
      if (sort.key === "size") c = a.size - b.size;
      else if (sort.key === "modified") c = (a.modified ?? 0) - (b.modified ?? 0);
      else if (sort.key === "kind") c = a.kind.localeCompare(b.kind);
      if (c === 0) c = a.name.localeCompare(b.name, undefined, { numeric: true, sensitivity: "base" });
      return c * dir;
    });
    return list;
  }, [rows, sort, filter]);

  // keep the keyboard anchor valid
  useEffect(() => {
    if (anchor.current && !rows.some((r) => r.key === anchor.current)) anchor.current = null;
  }, [rows]);

  function click(e: MouseEvent, row: Row) {
    const keys = shown.map((r) => r.key);
    if (e.shiftKey && anchor.current) {
      const a = keys.indexOf(anchor.current);
      const b = keys.indexOf(row.key);
      const [lo, hi] = a < b ? [a, b] : [b, a];
      const range = new Set(keys.slice(lo, hi + 1));
      if (e.ctrlKey || e.metaKey) selected.forEach((k) => range.add(k));
      onSelect(range);
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      const next = new Set(selected);
      if (next.has(row.key)) next.delete(row.key);
      else next.add(row.key);
      anchor.current = row.key;
      onSelect(next);
      return;
    }
    anchor.current = row.key;
    onSelect(new Set([row.key]));
  }

  function keyDown(e: KeyboardEvent) {
    const keys = shown.map((r) => r.key);
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && keys.length) {
      e.preventDefault();
      const cur = anchor.current ? keys.indexOf(anchor.current) : -1;
      const next = Math.max(0, Math.min(keys.length - 1, cur + (e.key === "ArrowDown" ? 1 : -1)));
      const k = keys[next];
      if (e.shiftKey) {
        const s = new Set(selected);
        s.add(k);
        onSelect(s);
      } else onSelect(new Set([k]));
      anchor.current = k;
      box.current?.querySelector(`[data-key="${CSS.escape(k)}"]`)?.scrollIntoView({ block: "nearest" });
      return;
    }
    if (e.key === "Home" || e.key === "End") {
      if (!keys.length) return;
      e.preventDefault();
      const k = e.key === "Home" ? keys[0] : keys[keys.length - 1];
      anchor.current = k;
      onSelect(new Set([k]));
      box.current?.querySelector(`[data-key="${CSS.escape(k)}"]`)?.scrollIntoView({ block: "nearest" });
      return;
    }
    if (e.key === "a" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      onSelect(new Set(keys));
      return;
    }
    if (e.key === "Enter" && selected.size === 1) {
      const row = shown.find((r) => selected.has(r.key));
      if (row) {
        e.preventDefault();
        onOpen(row);
        return;
      }
    }
    onKey?.(e);
  }

  const head = (key: SortKey, label: string, cls?: string) => (
    <th
      className={cls}
      onClick={() => setSort((s) => ({ key, asc: s.key === key ? !s.asc : true }))}
    >
      {label}
      {sort.key === key ? (sort.asc ? " ▲" : " ▼") : ""}
    </th>
  );

  return (
    <div
      className="list"
      ref={box}
      tabIndex={0}
      onKeyDown={keyDown}
      onContextMenu={(e) => {
        if ((e.target as HTMLElement).closest("tr[data-key]")) return;
        onSelect(new Set());
        onContext(e, null);
      }}
      onMouseDown={(e) => {
        if (e.button === 0 && !(e.target as HTMLElement).closest("tr")) onSelect(new Set());
      }}
    >
      {shown.length === 0 && empty ? (
        <div className="pane-empty">{empty}</div>
      ) : (
        <table>
          <colgroup>
            <col />
            <col className="c-size" />
            <col className="c-kind" />
            {showDate && <col className="c-date" />}
          </colgroup>
          <thead>
            <tr>
              {head("name", "Name")}
              {head("size", "Size", "num")}
              {head("kind", "Type", "col-kind")}
              {showDate && head("modified", "Modified", "col-date")}
            </tr>
          </thead>
          <tbody>
            {shown.map((row) => (
              <tr
                key={row.key}
                data-key={row.key}
                className={`${selected.has(row.key) ? "sel" : ""}${dropKey === row.key ? " drop" : ""}`}
                onMouseDown={(e) => {
                  if (e.button === 0 && !(selected.has(row.key) && !e.ctrlKey && !e.shiftKey && !e.metaKey)) click(e, row);
                }}
                onMouseUp={(e) => {
                  // a plain click on an already-selected row narrows to it (unless it became a drag)
                  if (e.button === 0 && selected.has(row.key) && !e.ctrlKey && !e.shiftKey && !e.metaKey && !press.current?.started) {
                    anchor.current = row.key;
                    if (selected.size > 1) onSelect(new Set([row.key]));
                  }
                }}
                onDoubleClick={() => onOpen(row)}
                onContextMenu={(e) => {
                  e.stopPropagation();
                  if (!selected.has(row.key)) {
                    anchor.current = row.key;
                    onSelect(new Set([row.key]));
                  }
                  onContext(e, row);
                }}
                onPointerDown={(e) => {
                  if (e.button === 0) press.current = { x: e.clientX, y: e.clientY, row, started: false };
                }}
                onPointerMove={(e) => {
                  const p = press.current;
                  if (!p || p.started || !onDragStart || e.buttons !== 1) return;
                  if (Math.hypot(e.clientX - p.x, e.clientY - p.y) > 6) {
                    p.started = true;
                    const keys = selected.has(p.row.key) ? selected : new Set([p.row.key]);
                    onDragStart(shown.filter((r) => keys.has(r.key)), e);
                  }
                }}
                onPointerUp={() => {
                  window.setTimeout(() => (press.current = null), 0);
                }}
              >
                <td className="name">
                  <RowIcon row={row} />
                  <span title={row.name}>{row.name}</span>
                </td>
                <td className="num dim">{row.isDir ? "" : formatSize(row.size)}</td>
                <td className="dim col-kind">{row.kind}</td>
                {showDate && <td className="dim col-date">{formatDate(row.modified)}</td>}
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
