// The handheld's screen, refreshed while visible.
import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from "react";
import { api, errorText, toImageData } from "../api";
import { useApp } from "../store";

export interface LiveScreenHandle {
  refresh: () => void;
  canvas: () => HTMLCanvasElement | null;
}

interface Props {
  /** Frames per second; 0 pauses. */
  fps: number;
  scale: number;
  smooth?: boolean;
  onFps?: (fps: number) => void;
}

export const LiveScreen = forwardRef<LiveScreenHandle, Props>(function LiveScreen({ fps, scale, smooth, onFps }, ref) {
  const { connected, jobs } = useApp();
  const canvas = useRef<HTMLCanvasElement>(null);
  const [hasFrame, setHasFrame] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const busy = useRef(false);
  const jobRunning = jobs.some((j) => j.state === "running");

  const grab = useCallback(async () => {
    if (busy.current || !connected) return;
    busy.current = true;
    try {
      const shot = await api.screenshot();
      const c = canvas.current;
      if (c) {
        if (c.width !== shot.width || c.height !== shot.height) {
          c.width = shot.width;
          c.height = shot.height;
        }
        c.getContext("2d")?.putImageData(toImageData(shot), 0, 0);
        setHasFrame(true);
        setError(null);
      }
    } catch (e) {
      setError(errorText(e));
    } finally {
      busy.current = false;
    }
  }, [connected]);

  useImperativeHandle(ref, () => ({ refresh: grab, canvas: () => canvas.current }), [grab]);

  useEffect(() => {
    if (!connected) {
      setHasFrame(false);
      return;
    }
    grab();
  }, [connected, grab]);

  // the refresh loop: as fast as asked, never overlapping, paused while a transfer runs
  useEffect(() => {
    if (!connected || fps <= 0 || jobRunning) {
      onFps?.(0);
      return;
    }
    let stop = false;
    let frames = 0;
    let since = performance.now();
    const loop = async () => {
      while (!stop) {
        const t0 = performance.now();
        await grab();
        frames++;
        const now = performance.now();
        if (now - since >= 1000) {
          onFps?.((frames * 1000) / (now - since));
          frames = 0;
          since = now;
        }
        const wait = Math.max(0, 1000 / fps - (performance.now() - t0));
        await new Promise((r) => setTimeout(r, wait));
        if (document.hidden) await new Promise((r) => setTimeout(r, 500));
      }
    };
    loop();
    return () => {
      stop = true;
    };
  }, [connected, fps, jobRunning, grab]); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <div className="lcd-frame">
      <canvas ref={canvas} width={320} height={240} style={{ width: 320 * scale, height: 240 * scale }} className={smooth ? "smooth" : ""} />
      {(!connected || !hasFrame || (error && !hasFrame)) && (
        <div className="lcd-off">{!connected ? "Connect a calculator to see its screen." : error ?? "Loading the screen…"}</div>
      )}
      {connected && jobRunning && fps > 0 && hasFrame && (
        <div className="lcd-off" style={{ inset: "auto 18px 18px auto", padding: "4px 8px", background: "rgba(0,0,0,.6)", borderRadius: 6, fontSize: 11.5 }}>
          Paused during a transfer
        </div>
      )}
    </div>
  );
});
