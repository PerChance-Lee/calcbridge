//! Build Notes pages (`TI.Notepad` ver 2.0, an "r2dtotree" rich-text tree) with real 2D math boxes.
//!
//! Paragraph markup: `**bold**`, text, `$math$` (`$!math$` = box with warning indicators off).
//! Math markup (LaTeX-like): `\frac{a}{b}`, `\sqrt{a}`, `x^{n}`/`x^2`, `x_{k+1}`/`x_k`, `( )`,
//! `\abs{a}`, `\hat{x}`. Letters are italic, digits/operators upright, known words upright.
//!
//! The node and template names mirror TI's own documents. Rendering facts checked on a CX II (OS 5.4):
//! the fonts lack `⋯` and combining/modifier hats (so `…` and a raised caret are used); an italic f hides
//! a prime drawn right after it (a no-break space is inserted); math boxes wrap at plain spaces.

use serde::{Deserialize, Serialize};

const NO_FILL: u32 = 268_435_199;
pub const TEXT_FONT: &str = "TI-Nspire Sans";
pub const MATH_FONT: &str = "TI-Nspire";
const UPRIGHT_WORDS: [&str; 13] =
    ["sin", "cos", "tan", "ln", "log", "lim", "max", "min", "abs", "fl", "DP", "exp", "det"];

fn substitute(c: char) -> char {
    match c {
        '⋯' => '…',
        '\u{2032}' => '\'',
        c => c,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct MathSyntaxError(pub String);

pub(crate) fn xml_escape(s: &str, quote: bool) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' if quote => out.push_str("&quot;"),
            '\'' if quote => out.push_str("&#x27;"),
            c => out.push(c),
        }
    }
    out
}

/// One paragraph of a Notes page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Paragraph {
    pub markup: String,
    #[serde(default)]
    pub color: u32,
    #[serde(default = "default_size")]
    pub size: u8,
    #[serde(default)]
    pub bold: bool,
    /// Take the text literally ($ and ** are not markup).
    #[serde(default)]
    pub raw: bool,
}

fn default_size() -> u8 {
    11
}

impl Paragraph {
    pub fn new(markup: impl Into<String>) -> Self {
        Paragraph { markup: markup.into(), size: 11, ..Default::default() }
    }
    pub fn title(markup: impl Into<String>, color: u32) -> Self {
        Paragraph { markup: markup.into(), size: 11, bold: true, color, ..Default::default() }
    }
    pub fn raw(text: impl Into<String>) -> Self {
        Paragraph { markup: text.into(), size: 11, raw: true, ..Default::default() }
    }
}

impl From<&str> for Paragraph {
    fn from(s: &str) -> Self {
        Paragraph::new(s)
    }
}

// ------------------------------------------------------------------ formats
#[derive(Debug, Clone, PartialEq, Eq)]
struct FormatKey {
    font: &'static str,
    style: u8,
    color: u32,
    size: u8,
    sub: bool,
    sup: bool,
}

struct Formats {
    keys: Vec<FormatKey>,
    refs: Vec<u32>,
}

impl Formats {
    fn new() -> Self {
        Formats {
            keys: vec![FormatKey { font: TEXT_FONT, style: 0, color: 0, size: 11, sub: false, sup: false }],
            refs: vec![2_147_483_647],
        }
    }

    fn get(&mut self, key: FormatKey) -> usize {
        let i = match self.keys.iter().position(|k| *k == key) {
            Some(i) => i,
            None => {
                self.keys.push(key);
                self.refs.push(0);
                self.keys.len() - 1
            }
        };
        if i != 0 {
            self.refs[i] += 1;
        }
        i
    }

    fn xml(&self) -> String {
        let mut out = String::new();
        for (i, k) in self.keys.iter().enumerate() {
            out.push_str(&format!(
                "<formatEntry entryIndex=\"{i}\" entryID=\"{i}\" entryRefCnt=\"{}\" tc=\"{}\" fc=\"{NO_FILL}\" fs=\"{}\" \
                 fst=\"{}\" cc=\"0\" fest=\"0\" feun=\"0\" fesub=\"{}\" fesup=\"{}\" fn0=\"{}\"></formatEntry>",
                self.refs[i],
                k.color,
                k.size,
                k.style,
                k.sub as u8,
                k.sup as u8,
                xml_escape(k.font, true)
            ));
        }
        let cap = 10.max(self.keys.len().div_ceil(10) * 10);
        format!("<formatManager tableSize=\"{}\" capacity=\"{cap}\">{out}</formatManager>", self.keys.len())
    }
}

fn leaf(name: &str, runs: &[(usize, String)]) -> String {
    let runs: Vec<&(usize, String)> = runs.iter().filter(|(_, t)| !t.is_empty()).collect();
    let empty = (0usize, String::new());
    let runs = if runs.is_empty() { vec![&empty] } else { runs };
    let mut attrs = Vec::new();
    let mut pos = 0usize;
    for (k, (f, t)) in runs.iter().enumerate() {
        pos += t.chars().count();
        attrs.push(format!("id{k}=\"{f}\" pp{k}=\"{pos}\""));
    }
    let ucf = if runs.iter().any(|(f, _)| *f != 0) { " ucf=\"1\"" } else { "" };
    let text: String = runs.iter().map(|(_, t)| t.as_str()).collect();
    let head = format!("<leaf name=\"{name}\" np=\"{}\" {}{ucf}", runs.len(), attrs.join(" "));
    if text.is_empty() { head + "/>" } else { format!("{head}>{}</leaf>", xml_escape(&text, false)) }
}

// --------------------------------------------------------------------- math
/// Parsed math markup.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "t", content = "v", rename_all = "lowercase")]
pub enum MathItem {
    Text(String),
    Sub(Vec<MathItem>),
    Sup(Vec<MathItem>),
    Pow(Vec<MathItem>),
    Frac(Vec<MathItem>, Vec<MathItem>),
    Sqrt(Vec<MathItem>),
    Paren(char),
}

/// `\name` commands that stand for one character.
pub const SYMBOLS: &[(&str, char)] = &[
    ("alpha", 'α'),
    ("beta", 'β'),
    ("gamma", 'γ'),
    ("delta", 'δ'),
    ("epsilon", 'ε'),
    ("varepsilon", 'ε'),
    ("zeta", 'ζ'),
    ("eta", 'η'),
    ("theta", 'θ'),
    ("iota", 'ι'),
    ("kappa", 'κ'),
    ("lambda", 'λ'),
    ("mu", 'μ'),
    ("nu", 'ν'),
    ("xi", 'ξ'),
    ("pi", 'π'),
    ("rho", 'ρ'),
    ("sigma", 'σ'),
    ("tau", 'τ'),
    ("phi", 'φ'),
    ("varphi", 'φ'),
    ("chi", 'χ'),
    ("psi", 'ψ'),
    ("omega", 'ω'),
    ("Gamma", 'Γ'),
    ("Delta", 'Δ'),
    ("Theta", 'Θ'),
    ("Lambda", 'Λ'),
    ("Pi", 'Π'),
    ("Sigma", 'Σ'),
    ("Phi", 'Φ'),
    ("Psi", 'Ψ'),
    ("Omega", 'Ω'),
    ("infty", '∞'),
    ("le", '≤'),
    ("leq", '≤'),
    ("ge", '≥'),
    ("geq", '≥'),
    ("ne", '≠'),
    ("neq", '≠'),
    ("pm", '±'),
    ("cdot", '·'),
    ("times", '×'),
    ("div", '÷'),
    ("to", '→'),
    ("rightarrow", '→'),
    ("approx", '≈'),
    ("degree", '°'),
    ("angle", '∠'),
    ("dots", '…'),
    ("ldots", '…'),
    ("cdots", '…'),
];

struct Parser {
    s: Vec<char>,
    i: usize,
}

impl Parser {
    fn group(&mut self) -> Result<Vec<MathItem>, MathSyntaxError> {
        let Some(&c) = self.s.get(self.i) else {
            return Err(MathSyntaxError("expected an argument at the end of the formula".into()));
        };
        if c == '{' {
            self.i += 1;
            let items = self.seq(Some('}'))?;
            if self.i >= self.s.len() {
                return Err(MathSyntaxError("missing }".into()));
            }
            self.i += 1;
            return Ok(items);
        }
        self.i += 1;
        Ok(vec![MathItem::Text(c.to_string())])
    }

    fn seq(&mut self, stop: Option<char>) -> Result<Vec<MathItem>, MathSyntaxError> {
        let mut items = Vec::new();
        while self.i < self.s.len() && Some(self.s[self.i]) != stop {
            let c = self.s[self.i];
            match c {
                '\\' => {
                    let mut j = self.i + 1;
                    while j < self.s.len() && self.s[j].is_alphabetic() {
                        j += 1;
                    }
                    let cmd: String = self.s[self.i + 1..j].iter().collect();
                    self.i = j;
                    match cmd.as_str() {
                        "frac" => {
                            let a = self.group()?;
                            let b = self.group()?;
                            items.push(MathItem::Frac(a, b));
                        }
                        "sqrt" => items.push(MathItem::Sqrt(self.group()?)),
                        "abs" => {
                            items.push(MathItem::Text("|".into()));
                            items.extend(self.group()?);
                            items.push(MathItem::Text("|".into()));
                        }
                        "hat" => {
                            items.extend(self.group()?);
                            items.push(MathItem::Sup(vec![MathItem::Text("^".into())]));
                        }
                        name => match SYMBOLS.iter().find(|(n, _)| *n == name) {
                            Some((_, ch)) => items.push(MathItem::Text(ch.to_string())),
                            None => return Err(MathSyntaxError(format!("unknown command \\{cmd}"))),
                        },
                    }
                }
                '^' => {
                    self.i += 1;
                    items.push(MathItem::Pow(self.group()?));
                }
                '_' => {
                    self.i += 1;
                    items.push(MathItem::Sub(self.group()?));
                }
                '(' | ')' => {
                    self.i += 1;
                    items.push(MathItem::Paren(c));
                }
                '\'' if matches!(items.last(), Some(MathItem::Text(t)) if t.chars().all(char::is_alphabetic) && !t.is_empty()) =>
                {
                    // the italic f's hook hides a prime drawn right after it: keep them apart
                    self.i += 1;
                    items.push(MathItem::Text("\u{a0}".into()));
                    items.push(MathItem::Text("'".into()));
                }
                '}' => return Err(MathSyntaxError("unexpected }".into())),
                _ => {
                    self.i += 1;
                    items.push(MathItem::Text(substitute(c).to_string()));
                }
            }
        }
        Ok(items)
    }
}

/// Parse math markup into an item tree.
pub fn parse_math(src: &str) -> Result<Vec<MathItem>, MathSyntaxError> {
    let mut p = Parser { s: src.chars().collect(), i: 0 };
    let items = p.seq(None)?;
    if p.i < p.s.len() {
        return Err(MathSyntaxError("unexpected }".into()));
    }
    Ok(items)
}

pub(crate) fn flatten(items: &[MathItem]) -> String {
    let mut out = String::new();
    for it in items {
        match it {
            MathItem::Text(t) => out.push_str(t),
            MathItem::Paren(c) => out.push(*c),
            MathItem::Frac(a, b) => out.push_str(&format!("({})/({})", flatten(a), flatten(b))),
            MathItem::Sqrt(a) => out.push_str(&format!("√({})", flatten(a))),
            MathItem::Pow(a) => out.push_str(&format!("^{}", flatten(a))),
            MathItem::Sub(a) | MathItem::Sup(a) => out.push_str(&flatten(a)),
        }
    }
    out
}

fn style_runs(text: &str, fmt: &mut Formats, sub: bool, sup: bool) -> Vec<(usize, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut runs: Vec<(usize, String)> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let word = UPRIGHT_WORDS.iter().find(|w| {
            let wc: Vec<char> = w.chars().collect();
            chars[i..].starts_with(&wc)
                && !(i + wc.len() < chars.len() && chars[i + wc.len()].is_alphabetic() && !matches!(**w, "fl" | "DP"))
        });
        let (piece, style) = match word {
            Some(w) => {
                i += w.chars().count();
                (w.to_string(), 0u8)
            }
            None => {
                let c = chars[i];
                i += 1;
                (c.to_string(), if c.is_alphabetic() { 2 } else { 0 })
            }
        };
        let f = fmt.get(FormatKey { font: MATH_FONT, style, color: 0, size: 11, sub, sup });
        match runs.last_mut() {
            Some((lf, lt)) if *lf == f => lt.push_str(&piece),
            _ => runs.push((f, piece)),
        }
    }
    runs
}

fn el(items: &[MathItem], fmt: &mut Formats) -> String {
    let mut parts: Vec<String> = Vec::new();
    let mut runs: Vec<(usize, String)> = Vec::new();
    for it in items {
        match it {
            MathItem::Text(t) => runs.extend(style_runs(t, fmt, false, false)),
            MathItem::Sub(a) => runs.extend(style_runs(&flatten(a), fmt, true, false)),
            MathItem::Sup(a) => runs.extend(style_runs(&flatten(a), fmt, false, true)),
            other => {
                parts.push(leaf("0text", &runs));
                runs.clear();
                parts.push(match other {
                    MathItem::Paren(c) => {
                        let data = if *c == '(' { 0u32 } else { 2_147_483_648 };
                        format!("<leaf name=\"0paren\" id0=\"0\" ucf=\"1\" data=\"{data}\">{c}</leaf>")
                    }
                    MathItem::Pow(a) => format!("<node name=\"0power\" id0=\"0\" ucf=\"1\">^({})</node>", el(a, fmt)),
                    MathItem::Frac(a, b) => {
                        format!("<node name=\"0fraction\" id0=\"0\">(({})/({}))</node>", el(a, fmt), el(b, fmt))
                    }
                    MathItem::Sqrt(a) => format!("<node name=\"0sqrt\" id0=\"0\">√({})</node>", el(a, fmt)),
                    _ => unreachable!(),
                });
            }
        }
    }
    parts.push(leaf("0text", &runs));
    format!("<node name=\"0el\" id0=\"0\">{}</node>", parts.concat())
}

fn math_box(src: &str, fmt: &mut Formats, warn: bool) -> Result<String, MathSyntaxError> {
    let items = parse_math(src)?;
    Ok(format!(
        "<node name=\"0el\" flags=\"8\" id0=\"0\"><mbox><attr iomode=\"1\" digits=\"0\" angle=\"0\" symbol=\"1\" \
         wrap=\"true\" warn=\"{warn}\" comp=\"1\" active=\"false\" id0=\"0\"/><out><err>0</err></out></mbox>{}</node>",
        el(&items, fmt)
    ))
}

// --------------------------------------------------------------- paragraphs
#[derive(Debug, Clone, PartialEq)]
pub enum Piece {
    Math(String),
    Text(String, bool),
}

/// Split paragraph markup into math and (bold/plain) text pieces.
pub fn split_markup(markup: &str) -> Result<Vec<Piece>, MathSyntaxError> {
    if markup.matches('$').count() % 2 == 1 {
        return Err(MathSyntaxError("a $ math section isn't closed".into()));
    }
    let mut pieces = Vec::new();
    for (k, chunk) in markup.split('$').enumerate() {
        if k % 2 == 1 {
            pieces.push(Piece::Math(chunk.to_string()));
            continue;
        }
        for (j, piece) in chunk.split("**").enumerate() {
            if !piece.is_empty() {
                pieces.push(Piece::Text(piece.to_string(), j % 2 == 1));
            }
        }
    }
    Ok(pieces)
}

fn words(text: &str, f: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if c == ' ' {
            out.push(leaf("1word", &[(f, std::mem::take(&mut cur))]));
        }
    }
    if !cur.is_empty() {
        out.push(leaf("1word", &[(f, cur)]));
    }
    out
}

fn paragraph(p: &Paragraph, fmt: &mut Formats) -> Result<String, MathSyntaxError> {
    let pieces = if p.raw { vec![Piece::Text(p.markup.clone(), false)] } else { split_markup(&p.markup)? };
    let empty = leaf("1word", &[(0, String::new())]);
    let mut leaves = Vec::new();
    for piece in pieces {
        match piece {
            Piece::Math(src) => {
                leaves.push(empty.clone());
                let (src, warn) = match src.strip_prefix('!') {
                    Some(rest) => (rest.to_string(), false),
                    None => (src, true),
                };
                leaves.push(math_box(&src, fmt, warn)?);
                leaves.push(empty.clone());
            }
            Piece::Text(text, bold) => {
                let style = (p.bold || bold) as u8;
                let f = if style != 0 || p.color != 0 || p.size != 11 {
                    fmt.get(FormatKey { font: TEXT_FONT, style, color: p.color, size: p.size, sub: false, sup: false })
                } else {
                    0
                };
                let text: String = text.chars().map(substitute).collect();
                leaves.extend(words(&text, f));
            }
        }
    }
    leaves.push(empty);
    Ok(format!("<node name=\"1para\"><node name=\"1rtline\">{}</node></node>", leaves.concat()))
}

/// One Notes page as a `<card>` element.
pub fn notes_card(paragraphs: &[Paragraph]) -> Result<String, MathSyntaxError> {
    let mut fmt = Formats::new();
    let default = [Paragraph::new("")];
    let paragraphs = if paragraphs.is_empty() { &default[..] } else { paragraphs };
    let mut body = Vec::new();
    for p in paragraphs {
        body.push(paragraph(p, &mut fmt)?);
    }
    // TI keeps a cursor marker in the first word.
    let first = &body[0];
    let i = first.find("<leaf name=\"1word\"").unwrap();
    let j = i + first[i..].find('>').unwrap();
    body[0] = if first.as_bytes()[j - 1] == b'/' {
        format!("{}><cursor index=\"0\"/></leaf>{}", &first[..j - 1], &first[j + 1..])
    } else {
        format!("{}<cursor index=\"0\"/>{}", &first[..=j], &first[j + 1..])
    };
    let tree =
        format!("<r2dtotree version=\"1\">{}<node name=\"1doc\">{}</node></r2dtotree>", fmt.xml(), body.concat());
    Ok(format!(
        "<card clay=\"0\" h1=\"10000\" h2=\"10000\" w1=\"10000\" w2=\"10000\"><isDummyCard>0</isDummyCard><flag>0</flag>\
         <wdgt xmlns:np=\"urn:TI.Notepad\" type=\"TI.Notepad\" ver=\"2.0\"><np:mFlags>0</np:mFlags><np:value>3</np:value>\
         <np:fmtxt>{}</np:fmtxt></wdgt></card>",
        xml_escape(&tree, true)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symbol_commands() {
        let items = parse_math(r"\alpha\le\pi\cdots x").unwrap();
        assert_eq!(flatten(&items), "α≤π… x");
        assert!(parse_math(r"\nosuch").is_err());
    }

    #[test]
    fn templates_and_quirks() {
        let card = notes_card(&[
            Paragraph::title("Title", 0x1F5FD0),
            Paragraph::new(r"**What:** $x_{k+1}=x_{k}-\frac{f(x_{k})}{f'(x_{k})}$ and $\sqrt{2}≈1.41$"),
            Paragraph::new(r"Hat $\hat{x}$, abs $\abs{x-y}$, ⋯ becomes $a+⋯+b^{2}$"),
        ])
        .unwrap();
        let doc = roxmltree::Document::parse(&card).unwrap();
        let tree = doc.descendants().find(|n| n.tag_name().name() == "fmtxt").unwrap().text().unwrap();
        roxmltree::Document::parse(tree).unwrap();
        for needle in ["0fraction", "0sqrt", "0power", "0paren", "\u{a0}'", "…", "fesup=\"1\"", "fesub=\"1\""] {
            assert!(tree.contains(needle), "missing {needle}");
        }
        assert!(!tree.contains('⋯'));
    }

    #[test]
    fn syntax_errors() {
        for bad in [r"\frac{1}", r"\nope{x}", "x^", "a}"] {
            assert!(parse_math(bad).is_err(), "{bad}");
        }
        assert!(notes_card(&[Paragraph::new("unclosed $x")]).is_err());
    }
}
