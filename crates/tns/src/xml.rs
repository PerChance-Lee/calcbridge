//! Parsing XML that comes from a document someone sent you.

/// Deeper than any real document (TI's Notes trees nest a few dozen levels), and far below the
/// nesting at which the recursive parsers overflow a 2 MB thread stack.
pub const MAX_DEPTH: usize = 256;

/// Roughly how deep the elements nest. Enough to refuse pathological input before a recursive
/// parser sees it; a `>` inside an attribute value can make it slightly off, which doesn't matter here.
fn depth_exceeds(xml: &str, limit: usize) -> bool {
    let b = xml.as_bytes();
    let (mut depth, mut i) = (0usize, 0usize);
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        match b.get(i + 1) {
            Some(b'/') => depth = depth.saturating_sub(1),
            Some(b'?' | b'!') => {}
            _ => {
                let end = b[i..].iter().position(|&c| c == b'>').map_or(b.len(), |p| i + p);
                if b.get(end.wrapping_sub(1)) != Some(&b'/') {
                    depth += 1;
                    if depth > limit {
                        return true;
                    }
                }
                i = end;
            }
        }
        i += 1;
    }
    false
}

/// Parse XML from an untrusted document, refusing nesting deep enough to exhaust the stack.
pub fn parse(xml: &str) -> Result<roxmltree::Document<'_>, String> {
    if depth_exceeds(xml, MAX_DEPTH) {
        return Err(format!("the XML nests more than {MAX_DEPTH} levels deep"));
    }
    roxmltree::Document::parse(xml).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_deep_nesting_but_not_documents() {
        let deep = "<a>".repeat(5000) + &"</a>".repeat(5000);
        assert!(parse(&deep).is_err());
        let fine = "<a><b x=\"1\"/><c>t</c></a>".repeat(100);
        assert!(!depth_exceeds(&fine, 3));
        assert!(parse("<r><a><b/></a></r>").is_ok());
    }

    #[test]
    fn deep_nesting_is_refused_on_a_small_stack() {
        // the regression: this used to overflow the stack of a 2 MB thread and kill the process
        let deep = "<node name=\"1para\">".repeat(3000) + &"</node>".repeat(3000);
        let handle = std::thread::Builder::new()
            .stack_size(2 << 20)
            .spawn(move || crate::r2d::parse_r2d(&deep).is_err())
            .unwrap();
        assert!(handle.join().unwrap());
    }
}
