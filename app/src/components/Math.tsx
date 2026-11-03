// Notes paragraphs (styled text runs and math boxes) rendered with KaTeX, offline.
import katex from "katex";
import { memo, useMemo } from "react";
import type { Span, ViewParagraph } from "../api";

const cache = new Map<string, string>();

function renderLatex(latex: string): string {
  let html = cache.get(latex);
  if (html === undefined) {
    // \displaystyle: the handheld draws fractions in math boxes at full size, not shrunk like inline TeX
    html = katex.renderToString(String.raw`\displaystyle ` + latex, { throwOnError: false, strict: "ignore", output: "html", trust: false });
    if (cache.size > 2000) cache.clear();
    cache.set(latex, html);
  }
  return html;
}

function SpanView({ span }: { span: Span }) {
  if (span.kind === "math") {
    return <span className="mathbox" dangerouslySetInnerHTML={{ __html: renderLatex(span.latex) }} />;
  }
  const s = span.style;
  let node = <>{span.text}</>;
  if (s.sub) node = <sub>{node}</sub>;
  if (s.sup) node = <sup>{node}</sup>;
  return (
    <span
      style={{
        fontWeight: s.bold ? 700 : undefined,
        fontStyle: s.italic ? "italic" : undefined,
        color: s.color,
        whiteSpace: "pre-wrap",
      }}
    >
      {node}
    </span>
  );
}

export const Paragraphs = memo(function Paragraphs({ paragraphs }: { paragraphs: ViewParagraph[] }) {
  return (
    <>
      {paragraphs.map((p, i) => (
        <p key={i}>
          {p.length === 0 ? " " : p.map((s, j) => <SpanView key={j} span={s} />)}
        </p>
      ))}
    </>
  );
});

/** One LaTeX formula, for help texts. */
export function Formula({ latex }: { latex: string }) {
  const html = useMemo(() => renderLatex(latex), [latex]);
  return <span dangerouslySetInnerHTML={{ __html: html }} />;
}
