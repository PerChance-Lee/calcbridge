//! Notes pages → a display model for the app: paragraphs of styled text runs and math boxes, with the
//! math as LaTeX (rendered with KaTeX in the UI). Works for TI-made documents and for markup previews.

use serde::Serialize;

use crate::notes::{MathItem, MathSyntaxError, Piece, parse_math, split_markup};

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct Style {
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sub: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub sup: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Span {
    Text { text: String, style: Style },
    Math { latex: String },
}

pub type ViewParagraph = Vec<Span>;

// ------------------------------------------------------------------- LaTeX
const OPERATORS: [&str; 13] = ["sin", "cos", "tan", "ln", "log", "lim", "max", "min", "abs", "exp", "det", "fl", "DP"];

fn latex_char(c: char, out: &mut String) {
    match c {
        '{' => out.push_str("\\{"),
        '}' => out.push_str("\\}"),
        '\\' => out.push_str("\\backslash "),
        '%' | '#' | '&' | '$' | '_' => {
            out.push('\\');
            out.push(c);
        }
        '^' => out.push_str("{\\wedge}"),
        '~' => out.push_str("\\sim "),
        '\u{a0}' => out.push_str("\\,"),
        ' ' => out.push_str("\\ "),
        '…' => out.push_str("\\ldots "),
        '·' => out.push_str("\\cdot "),
        '×' => out.push_str("\\times "),
        '÷' => out.push_str("\\div "),
        '−' => out.push('-'),
        c => out.push(c),
    }
}

/// Plain math text with TI's styling rules: known words upright (operators), everything else as is.
fn latex_text(s: &str, out: &mut String) {
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if let Some(w) = OPERATORS.iter().find(|w| {
            let wc: Vec<char> = w.chars().collect();
            chars[i..].starts_with(&wc)
                && !(i + wc.len() < chars.len() && chars[i + wc.len()].is_alphabetic() && !matches!(**w, "fl" | "DP"))
        }) {
            out.push_str(&format!("\\operatorname{{{w}}}"));
            i += w.chars().count();
            continue;
        }
        latex_char(chars[i], out);
        i += 1;
    }
}

fn parens_balanced(items: &[MathItem]) -> bool {
    let mut depth = 0i32;
    for it in items {
        if let MathItem::Paren(c) = it {
            depth += if *c == '(' { 1 } else { -1 };
            if depth < 0 {
                return false;
            }
        }
    }
    depth == 0
}

/// Math markup items → LaTeX for KaTeX.
pub fn items_to_latex(items: &[MathItem]) -> String {
    let mut out = String::new();
    let stretch = parens_balanced(items);
    for it in items {
        match it {
            MathItem::Text(t) => latex_text(t, &mut out),
            MathItem::Paren(c) => out.push_str(match (*c, stretch) {
                ('(', true) => "\\left(",
                (')', true) => "\\right)",
                ('(', false) => "(",
                _ => ")",
            }),
            MathItem::Sub(a) => out.push_str(&format!("_{{{}}}", items_to_latex(a))),
            MathItem::Sup(a) | MathItem::Pow(a) => out.push_str(&format!("^{{{}}}", items_to_latex(a))),
            MathItem::Frac(a, b) => out.push_str(&format!("\\frac{{{}}}{{{}}}", items_to_latex(a), items_to_latex(b))),
            MathItem::Sqrt(a) => out.push_str(&format!("\\sqrt{{{}}}", items_to_latex(a))),
        }
    }
    out
}

/// Paragraph markup (the Create tab's notation) → display model, for live previews.
pub fn markup_to_view(markup: &str) -> Result<ViewParagraph, MathSyntaxError> {
    let mut spans = Vec::new();
    for piece in split_markup(markup)? {
        match piece {
            Piece::Math(src) => {
                let items = parse_math(src.strip_prefix('!').unwrap_or(&src))?;
                spans.push(Span::Math { latex: items_to_latex(&items) });
            }
            Piece::Text(text, bold) => spans.push(Span::Text { text, style: Style { bold, ..Default::default() } }),
        }
    }
    Ok(spans)
}

// ------------------------------------------------------------- r2dtotree
#[derive(Clone, Default)]
struct Fmt {
    bold: bool,
    italic: bool,
    sub: bool,
    sup: bool,
    color: Option<String>,
}

fn tc_color(v: &str) -> Option<String> {
    let n: u32 = v.parse().ok()?;
    if n <= 1 { None } else { Some(format!("#{:06x}", n & 0xFF_FFFF)) }
}

fn runs<'a>(leaf: roxmltree::Node<'a, 'a>, formats: &std::collections::HashMap<String, Fmt>) -> Vec<(String, Fmt)> {
    let text: String = leaf.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect();
    let chars: Vec<char> = text.chars().collect();
    let np: usize = leaf.attribute("np").and_then(|v| v.parse().ok()).unwrap_or(1);
    let mut out = Vec::new();
    let mut start = 0usize;
    for k in 0..np {
        let end: usize = leaf.attribute(format!("pp{k}").as_str()).and_then(|v| v.parse().ok()).unwrap_or(chars.len());
        let end = end.min(chars.len());
        let fmt = formats.get(leaf.attribute(format!("id{k}").as_str()).unwrap_or("0")).cloned().unwrap_or_default();
        if end > start {
            out.push((chars[start..end].iter().collect(), fmt));
        }
        start = start.max(end);
    }
    if start < chars.len() {
        let fmt = formats.get(leaf.attribute("id0").unwrap_or("0")).cloned().unwrap_or_default();
        out.push((chars[start..].iter().collect(), fmt));
    }
    out
}

fn math_latex(el: roxmltree::Node, formats: &std::collections::HashMap<String, Fmt>) -> String {
    // collect parens first to decide whether they can stretch
    let kids: Vec<_> = el.children().filter(|n| n.is_element()).collect();
    let mut depth = 0i32;
    let mut ok = true;
    for k in &kids {
        if k.attribute("name") == Some("0paren") {
            let t: String = k.descendants().filter_map(|n| n.text()).collect();
            depth += if t.contains('(') { 1 } else { -1 };
            ok &= depth >= 0;
        }
    }
    let stretch = ok && depth == 0;
    let mut out = String::new();
    for k in kids {
        let name = k.attribute("name").unwrap_or("");
        let sub_els: Vec<_> = k.children().filter(|n| n.is_element() && n.attribute("name") == Some("0el")).collect();
        match (k.tag_name().name(), name) {
            ("leaf", "0text") => {
                let mut sub = String::new();
                let mut sup = String::new();
                let flush = |out: &mut String, sub: &mut String, sup: &mut String| {
                    if !sub.is_empty() {
                        out.push_str(&format!("_{{{sub}}}"));
                        sub.clear();
                    }
                    if !sup.is_empty() {
                        out.push_str(&format!("^{{{sup}}}"));
                        sup.clear();
                    }
                };
                for (text, f) in runs(k, formats) {
                    if f.sub {
                        latex_text(&text, &mut sub);
                    } else if f.sup {
                        latex_text(&text, &mut sup);
                    } else {
                        flush(&mut out, &mut sub, &mut sup);
                        latex_text(&text, &mut out);
                    }
                }
                flush(&mut out, &mut sub, &mut sup);
            }
            ("leaf", "0paren") => {
                let t: String = k.descendants().filter_map(|n| n.text()).collect();
                let open = t.contains('(');
                out.push_str(match (open, stretch) {
                    (true, true) => "\\left(",
                    (false, true) => "\\right)",
                    (true, false) => "(",
                    (false, false) => ")",
                });
            }
            ("node", "0fraction") if sub_els.len() >= 2 => out.push_str(&format!(
                "\\frac{{{}}}{{{}}}",
                math_latex(sub_els[0], formats),
                math_latex(sub_els[1], formats)
            )),
            ("node", "0sqrt") if !sub_els.is_empty() => {
                out.push_str(&format!("\\sqrt{{{}}}", math_latex(sub_els[0], formats)))
            }
            ("node", "0power") if !sub_els.is_empty() => {
                out.push_str(&format!("^{{{}}}", math_latex(sub_els[0], formats)))
            }
            ("node", "0el") => out.push_str(&math_latex(k, formats)),
            ("node", _) => {
                // a template we don't draw: show its linear text
                let t: String = k.descendants().filter(|n| n.is_text()).filter_map(|n| n.text()).collect();
                latex_text(&t, &mut out);
            }
            _ => {}
        }
    }
    out
}

fn collect_para(node: roxmltree::Node, formats: &std::collections::HashMap<String, Fmt>, spans: &mut ViewParagraph) {
    for child in node.children().filter(|n| n.is_element()) {
        let name = child.attribute("name").unwrap_or("");
        if child.tag_name().name() == "leaf" && name == "1word" {
            for (text, f) in runs(child, formats) {
                let style = Style { bold: f.bold, italic: f.italic, sub: f.sub, sup: f.sup, color: f.color };
                match spans.last_mut() {
                    Some(Span::Text { text: t, style: s }) if *s == style => t.push_str(&text),
                    _ => spans.push(Span::Text { text, style }),
                }
            }
        } else if child.tag_name().name() == "node" && name == "0el" && child.attribute("flags") == Some("8") {
            let inner = child.children().find(|n| n.is_element() && n.attribute("name") == Some("0el"));
            spans.push(Span::Math { latex: inner.map(|n| math_latex(n, formats)).unwrap_or_default() });
        } else if child.tag_name().name() == "node" {
            collect_para(child, formats, spans);
        }
    }
}

/// A Notes page's r2dtotree XML → paragraphs.
pub fn parse_r2d(tree_xml: &str) -> Result<Vec<ViewParagraph>, String> {
    let doc = roxmltree::Document::parse(tree_xml).map_err(|e| e.to_string())?;
    let mut formats = std::collections::HashMap::new();
    for fe in doc.descendants().filter(|n| n.has_tag_name("formatEntry")) {
        let fst = fe.attribute("fst").unwrap_or("0");
        formats.insert(
            fe.attribute("entryID").unwrap_or("").to_string(),
            Fmt {
                bold: matches!(fst, "1" | "3"),
                italic: matches!(fst, "2" | "3"),
                sub: fe.attribute("fesub") == Some("1"),
                sup: fe.attribute("fesup") == Some("1"),
                color: tc_color(fe.attribute("tc").unwrap_or("0")),
            },
        );
    }
    let Some(root) = doc.descendants().find(|n| n.attribute("name") == Some("1doc")) else {
        return Ok(Vec::new());
    };
    let mut paragraphs = Vec::new();
    for para in root.children().filter(|n| n.attribute("name") == Some("1para")) {
        let mut spans = Vec::new();
        collect_para(para, &formats, &mut spans);
        paragraphs.push(spans);
    }
    Ok(paragraphs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{TnsDocument, build_notes};
    use crate::notes::Paragraph;

    #[test]
    fn markup_preview_latex() {
        let spans = markup_to_view(r"**Newton:** $x_{k+1}=x_{k}-\frac{f(x_{k})}{f'(x_{k})}$").unwrap();
        assert_eq!(spans[0], Span::Text { text: "Newton:".into(), style: Style { bold: true, ..Default::default() } });
        let Span::Math { latex } = &spans[2] else { panic!() };
        assert_eq!(latex, "x_{k+1}=x_{k}-\\frac{f\\left(x_{k}\\right)}{f\\,'\\left(x_{k}\\right)}");
    }

    #[test]
    fn roundtrip_through_a_document() {
        let blob = build_notes(&[vec![
            Paragraph::title("Title", 0x1F5FD0),
            Paragraph::new(r"Root $\sqrt{1+x}$ and $\frac{a}{b}$, $\hat{x}$, $2^{−52}$"),
        ]])
        .unwrap();
        let doc = TnsDocument::from_bytes(&blob).unwrap();
        let paras = parse_r2d(&doc.notes_trees()[0].2).unwrap();
        assert_eq!(paras.len(), 2);
        assert!(
            matches!(&paras[0][0], Span::Text { text, style } if text == "Title" && style.bold && style.color.is_some())
        );
        let math: Vec<String> = paras[1]
            .iter()
            .filter_map(|s| if let Span::Math { latex } = s { Some(latex.clone()) } else { None })
            .collect();
        assert_eq!(math[0], "\\sqrt{1+x}");
        assert_eq!(math[1], "\\frac{a}{b}");
        assert_eq!(math[2], "x^{{\\wedge}}");
        assert_eq!(math[3], "2^{-52}");
    }
}
