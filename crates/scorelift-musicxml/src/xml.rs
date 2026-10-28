//! A minimal indenting XML writer. MusicXML needs only elements, attributes
//! and text, and a hand-rolled writer keeps the output byte-for-byte stable.

use std::fmt::Write;

#[derive(Debug, Default)]
pub(crate) struct XmlWriter {
    out: String,
    stack: Vec<&'static str>,
}

impl XmlWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn raw_line(&mut self, line: &str) {
        self.out.push_str(line);
        self.out.push('\n');
    }

    fn indent(&mut self) {
        for _ in 0..self.stack.len() {
            self.out.push_str("  ");
        }
    }

    fn start_tag(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.indent();
        self.out.push('<');
        self.out.push_str(name);
        for (k, v) in attrs {
            let _ = write!(self.out, " {k}=\"{}\"", escape(v));
        }
    }

    /// Opens an element whose children follow.
    pub fn open(&mut self, name: &'static str, attrs: &[(&str, &str)]) {
        self.start_tag(name, attrs);
        self.out.push_str(">\n");
        self.stack.push(name);
    }

    pub fn close(&mut self) {
        let name = self.stack.pop().expect("close without open");
        self.indent();
        let _ = writeln!(self.out, "</{name}>");
    }

    /// Writes `<name attrs/>`.
    pub fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.start_tag(name, attrs);
        self.out.push_str("/>\n");
    }

    /// Writes `<name attrs>text</name>`.
    pub fn text(&mut self, name: &str, attrs: &[(&str, &str)], text: impl std::fmt::Display) {
        self.start_tag(name, attrs);
        let _ = writeln!(self.out, ">{}</{name}>", escape(&text.to_string()));
    }

    pub fn finish(self) -> String {
        assert!(self.stack.is_empty(), "unclosed elements: {:?}", self.stack);
        self.out
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Control characters are not allowed in XML 1.0.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_nested_escaped_xml() {
        let mut w = XmlWriter::new();
        w.open("a", &[("x", "1 < 2")]);
        w.text("b", &[], "Tom & \"Jerry\"");
        w.empty("c", &[]);
        w.close();
        assert_eq!(
            w.finish(),
            "<a x=\"1 &lt; 2\">\n  <b>Tom &amp; &quot;Jerry&quot;</b>\n  <c/>\n</a>\n"
        );
    }
}
