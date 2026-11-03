import { useEffect, useRef, useState } from "react";

/** The largest scale (up to `max`) at which a w×h picture plus `pad` fits in the element. */
export function useFitScale<T extends HTMLElement>(w: number, h: number, pad: number, max: number) {
  const ref = useRef<T>(null);
  const [scale, setScale] = useState(1);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const s = Math.min((el.clientWidth - pad) / w, (el.clientHeight - pad) / h, max);
      setScale(Math.max(0.5, s));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [w, h, pad, max]);
  return [ref, scale] as const;
}
