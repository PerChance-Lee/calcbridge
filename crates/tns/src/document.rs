//! Create and inspect TI-Nspire documents.

use serde::Serialize;

use crate::container::{self, DEFAULT_VERSION, Entry, Method};
use crate::notes::{MathSyntaxError, Paragraph, notes_card, xml_escape};
use crate::{Result, TnsError};

/// Friendly names for the widget types found in documents.
pub fn app_name(widget_type: &str) -> String {
    match widget_type {
        "TI.Notepad" => "Notes",
        "TI.Calculator" => "Calculator",
        "TI.Scratchpad" => "Scratchpad",
        "TI.GeoGrapher" => "Graphs & Geometry",
        "TI.DataGrapher" => "Data & Statistics",
        "TI.ListsSpreadsheet" | "TI.Spreadsheet" => "Lists & Spreadsheet",
        "TI.PythonEditor" => "Python editor",
        "TI.PythonShell" => "Python shell",
        "TI.ScriptApp" => "Lua script",
        "TI.Question" => "Question",
        "TI.DataQuest" => "Vernier DataQuest",
        "TI.ChemBox" => "Chemistry",
        other => other,
    }
    .to_string()
}

/// Today's civil date (UTC), without pulling in a date crate.
fn today() -> (i64, u32, u32) {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64 / 86_400)
        .unwrap_or(0);
    // Howard Hinnant's days_from_civil inverse
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// `Document.xml` with the settings TI uses for a new non-CAS document.
pub fn document_xml(problems: usize) -> String {
    let (y, m, d) = today();
    format!(
        "<doc ver=\"1.0\"><properties><product>16</product><platform>2</platform><swver>5.4.0 CalcBridge</swver>\
         <date>{y}-{m}-{d}</date></properties><settings><assessmentMode>0</assessmentMode><lastview>1</lastview>\
         <readOnlyMode>0</readOnlyMode><lang>en</lang><dfmt>0</dfmt><tfmt>0</tfmt><curr>0</curr><expf>1</expf>\
         <ddig>7</ddig><angf>1</angf><exapp>1</exapp><cplxf>1</cplxf><unit>1</unit><vectf>1</vectf><base>1</base>\
         <gg_ddig>4</gg_ddig><gg_graphang>1</gg_graphang><gg_geomang>2</gg_geomang><gg_axisendv>1</gg_axisendv>\
         <gg_tooltipfunman>0</gg_tooltipfunman><gg_autopoi>1</gg_autopoi><gg_calcmenu>0</gg_calcmenu>\
         <gg_hideplotlabels>0</gg_hideplotlabels></settings><cardIntendedData><cardIntendedWidth>320</cardIntendedWidth>\
         <cardIntendedHeight>217</cardIntendedHeight><cardIntendedDensity>125.000000</cardIntendedDensity>\
         </cardIntendedData><rights></rights><nps>{problems}</nps><imgsize>0</imgsize></doc>"
    )
}

fn problem(cards: &[String]) -> String {
    format!("<prob xmlns=\"urn:TI.Problem\" ver=\"1.0\" pbname=\"\"><sym></sym>{}</prob>", cards.concat())
}

fn card(widget: &str) -> String {
    format!(
        "<card clay=\"0\" h1=\"10000\" h2=\"10000\" w1=\"10000\" w2=\"10000\"><isDummyCard>0</isDummyCard>\
         <flag>0</flag>{widget}</card>"
    )
}

fn invalid(e: MathSyntaxError) -> TnsError {
    TnsError::Invalid(e.0)
}

/// A Notes document: one page per element of `pages`.
pub fn build_notes(pages: &[Vec<Paragraph>]) -> Result<Vec<u8>> {
    let cards = pages.iter().map(|p| notes_card(p)).collect::<std::result::Result<Vec<_>, _>>().map_err(invalid)?;
    container::write(
        &[Entry::xml("Document.xml", document_xml(1)), Entry::xml("Problem1.xml", problem(&cards))],
        DEFAULT_VERSION,
    )
}

/// Is this a valid Python file name for the handheld (ASCII identifier + `.py`)?
pub fn valid_python_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".py") else { return false };
    let mut chars = stem.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// A module name the handheld accepts, derived from any document name.
pub fn safe_identifier(name: &str) -> String {
    let s: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let s = s.trim_matches('_');
    let s = if s.is_empty() { "program" } else { s };
    if s.starts_with(|c: char| c.is_ascii_digit()) { format!("p_{s}") } else { s.to_string() }
}

/// A Python program document; `script_name` opens first in the handheld's Python editor.
pub fn build_python(script_name: &str, code: &str, extra: &[(&str, &str)]) -> Result<Vec<u8>> {
    if !valid_python_name(script_name) {
        return Err(TnsError::Invalid(
            "Python file names must be a valid identifier ending in .py (e.g. my_prog.py)".into(),
        ));
    }
    let widget = format!(
        "<wdgt xmlns:py=\"urn:TI.PythonEditor\" type=\"TI.PythonEditor\" ver=\"1.0\"><py:data><py:name>{}</py:name>\
         <py:dirf>0</py:dirf></py:data><py:mFlags>0</py:mFlags><py:value>10</py:value></wdgt>",
        xml_escape(script_name, true)
    );
    let mut entries = vec![
        Entry::xml("Document.xml", document_xml(1)),
        Entry::xml("Problem1.xml", problem(&[card(&widget)])),
        Entry::file(script_name, code.replace("\r\n", "\n").into_bytes()),
    ];
    for (name, src) in extra {
        entries.push(Entry::file(name, src.replace("\r\n", "\n").into_bytes()));
    }
    container::write(&entries, DEFAULT_VERSION)
}

/// A Lua script document (TI.ScriptApp, API level 2).
pub fn build_lua(code: &str) -> Result<Vec<u8>> {
    let code = code.replace("]]>", "]]]]><![CDATA[>");
    let widget = format!(
        "<wdgt xmlns:sc=\"urn:TI.ScriptApp\" type=\"TI.ScriptApp\" ver=\"1.0\"><sc:mFlags>0</sc:mFlags>\
         <sc:value>-1</sc:value><sc:script version=\"512\" id=\"0\"><![CDATA[{code}]]></sc:script></wdgt>"
    );
    container::write(
        &[Entry::xml("Document.xml", document_xml(1)), Entry::xml("Problem1.xml", problem(&[card(&widget)]))],
        DEFAULT_VERSION,
    )
}

/// Plain text → one Notes page (no markup interpretation).
pub fn build_text(text: &str, title: Option<&str>) -> Result<Vec<u8>> {
    let mut paras = Vec::new();
    if let Some(t) = title {
        paras.push(Paragraph { markup: t.into(), size: 11, bold: true, color: 0x1F5FD0, raw: true });
    }
    paras.extend(text.replace("\r\n", "\n").split('\n').map(Paragraph::raw));
    build_notes(&[paras])
}

// ---------------------------------------------------------------- inspection
#[derive(Debug, Clone, Serialize)]
pub struct Page {
    pub problem: usize,
    pub index: usize,
    pub apps: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct TnsDocument {
    pub version: u16,
    pub entries: Vec<Entry>,
}

impl TnsDocument {
    pub fn from_bytes(blob: &[u8]) -> Result<Self> {
        let (version, entries) = container::read(blob)?;
        Ok(TnsDocument { version, entries })
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Problem XML entry names in order.
    pub fn problems(&self) -> Vec<String> {
        let mut names: Vec<(u32, String)> = self
            .entries
            .iter()
            .filter_map(|e| {
                let n = e.name.strip_prefix("Problem")?.strip_suffix(".xml")?.parse().ok()?;
                Some((n, e.name.clone()))
            })
            .collect();
        names.sort();
        names.into_iter().map(|(_, n)| n).collect()
    }

    fn with_problems<T>(&self, mut f: impl FnMut(usize, &roxmltree::Document) -> Vec<T>) -> Vec<T> {
        let mut out = Vec::new();
        for (pi, name) in self.problems().iter().enumerate() {
            let text = self.entry(name).map(|e| e.text()).unwrap_or_default();
            if let Ok(doc) = roxmltree::Document::parse(&text) {
                out.extend(f(pi + 1, &doc));
            }
        }
        out
    }

    pub fn pages(&self) -> Vec<Page> {
        self.with_problems(|pi, doc| {
            doc.descendants()
                .filter(|n| n.tag_name().name() == "card")
                .enumerate()
                .map(|(ci, card)| Page {
                    problem: pi,
                    index: ci + 1,
                    apps: card
                        .descendants()
                        .filter(|n| n.tag_name().name() == "wdgt")
                        .map(|w| app_name(w.attribute("type").unwrap_or("?")))
                        .collect(),
                })
                .collect()
        })
    }

    /// (problem, page, r2dtotree XML) for every Notes widget.
    pub fn notes_trees(&self) -> Vec<(usize, usize, String)> {
        self.with_problems(|pi, doc| {
            let mut out = Vec::new();
            for (ci, card) in doc.descendants().filter(|n| n.tag_name().name() == "card").enumerate() {
                for n in card.descendants().filter(|n| n.tag_name().name() == "fmtxt") {
                    if let Some(t) = n.text() {
                        out.push((pi, ci + 1, t.to_string()));
                    }
                }
            }
            out
        })
    }

    pub fn python_files(&self) -> Vec<(String, String)> {
        self.entries
            .iter()
            .filter(|e| e.name.ends_with(".py") && e.method != Method::Ti)
            .map(|e| (e.name.clone(), e.text()))
            .collect()
    }

    pub fn lua_script(&self) -> Option<String> {
        self.with_problems(|_, doc| {
            doc.descendants()
                .filter(|n| n.tag_name().name() == "script")
                .filter_map(|n| n.text().map(str::to_string))
                .collect()
        })
        .into_iter()
        .next()
    }

    pub fn summary(&self) -> String {
        let mut lines = vec![format!("TI-Nspire document, format version {:04X}", self.version)];
        for p in self.pages() {
            let apps = if p.apps.is_empty() { "empty".to_string() } else { p.apps.join(", ") };
            lines.push(format!("  Problem {}, page {}: {apps}", p.problem, p.index));
        }
        for (name, _) in self.python_files() {
            lines.push(format!("  Python file: {name}"));
        }
        let others: Vec<_> =
            self.entries.iter().filter(|e| !e.name.ends_with(".xml") && !e.name.ends_with(".py")).map(|e| e.name.as_str()).collect();
        if !others.is_empty() {
            lines.push(format!("  Other files: {}", others.join(", ")));
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_document() {
        let blob = build_notes(&[
            vec![Paragraph::title("Title", 0x1F5FD0), Paragraph::new(r"$\frac{1}{2}+\sqrt{x}$")],
            vec![Paragraph::new("Second page")],
        ])
        .unwrap();
        let doc = TnsDocument::from_bytes(&blob).unwrap();
        let apps: Vec<_> = doc.pages().into_iter().map(|p| p.apps).collect();
        assert_eq!(apps, vec![vec!["Notes".to_string()], vec!["Notes".to_string()]]);
        let trees = doc.notes_trees();
        assert_eq!(trees.len(), 2);
        assert!(trees[0].2.contains("0fraction"));
        roxmltree::Document::parse(&trees[0].2).unwrap();
    }

    #[test]
    fn python_and_lua() {
        let blob = build_python("hello.py", "print('hi')\r\n", &[("helper.py", "X = 1\n")]).unwrap();
        let doc = TnsDocument::from_bytes(&blob).unwrap();
        assert_eq!(
            doc.python_files(),
            vec![("hello.py".into(), "print('hi')\n".into()), ("helper.py".into(), "X = 1\n".into())]
        );
        assert_eq!(doc.pages()[0].apps, vec!["Python editor".to_string()]);
        assert!(build_python("1bad name.py", "", &[]).is_err());
        let lua = TnsDocument::from_bytes(&build_lua("gc:drawString(\"a]]>b\", 1, 1)").unwrap()).unwrap();
        assert_eq!(lua.pages()[0].apps, vec!["Lua script".to_string()]);
        assert!(lua.lua_script().unwrap().contains("a]]>b"));
    }

    #[test]
    fn text_document_is_literal() {
        let doc = TnsDocument::from_bytes(&build_text("line with $ and **stars**\nsecond", Some("T")).unwrap()).unwrap();
        let tree = &doc.notes_trees()[0].2;
        assert!(tree.contains(">$ </leaf>") && tree.contains(">**stars**</leaf>") && !tree.contains("0el"));
    }

    #[test]
    fn identifiers() {
        let id = safe_identifier("Résumé 2");
        assert!(valid_python_name(&format!("{id}.py")), "{id}");
        assert_eq!(safe_identifier("123"), "p_123");
        assert_eq!(safe_identifier("  "), "program");
    }
}
