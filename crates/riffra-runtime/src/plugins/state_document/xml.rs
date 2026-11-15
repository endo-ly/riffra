//! A span-preserving XML reader and editor.
//!
//! Plugin state documents are hundreds of kilobytes of text whose exact formatting belongs to the
//! plugin, so edits splice the source text instead of serializing a parsed document.

use std::collections::BTreeMap;
use std::ops::Range;

/// Failure to address or change a value inside an XML document.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum XmlEditError {
    #[error("xml path {0:?} was not found")]
    NotFound(String),
    #[error("xml path {0:?} is invalid")]
    InvalidPath(String),
    #[error("xml element {0:?} has child elements and cannot hold text")]
    HasChildElements(String),
    #[error("two xml edits change the same text")]
    Overlap,
    #[error("edited xml is not well-formed")]
    Malformed,
}

#[derive(Debug)]
struct Element {
    name: Range<usize>,
    children: Vec<usize>,
    /// Offset just past the name and attributes, before `/>` or `>`.
    attributes_end: usize,
    /// Offset just past the opening tag.
    open_end: usize,
    /// Span of the closing tag; `None` for a self-closing element.
    close: Option<Range<usize>>,
}

/// One parsed XML document borrowed from its source text.
#[derive(Debug)]
pub(super) struct Document<'a> {
    source: &'a [u8],
    elements: Vec<Element>,
    end: usize,
}

/// The value found at an XML path.
#[derive(Debug, Eq, PartialEq)]
pub(super) enum XmlValue {
    Text(String),
    Markup(String),
}

impl<'a> Document<'a> {
    /// Parses the first root element in `source`; anything after it is ignored.
    pub(super) fn parse(source: &'a [u8]) -> Option<Self> {
        Parser {
            source,
            position: 0,
            elements: Vec::new(),
        }
        .document()
    }

    /// Returns the offset just past the root element.
    pub(super) fn end(&self) -> usize {
        self.end
    }

    /// Returns the root element's name.
    pub(super) fn root_name(&self) -> String {
        self.text(self.elements[0].name.clone())
    }

    fn text(&self, span: Range<usize>) -> String {
        String::from_utf8_lossy(&self.source[span]).into_owned()
    }

    fn child(&self, parent: usize, segment: &Segment<'_>) -> Option<usize> {
        self.elements[parent]
            .children
            .iter()
            .copied()
            .filter(|child| {
                &self.source[self.elements[*child].name.clone()] == segment.name.as_bytes()
            })
            .nth(segment.index.unwrap_or(0))
    }

    fn element(&self, segments: &[Segment<'_>]) -> Option<usize> {
        segments
            .iter()
            .try_fold(0, |current, segment| self.child(current, segment))
    }

    fn attribute_value(&self, element: usize, name: &str) -> Option<Range<usize>> {
        let element = &self.elements[element];
        let end = element.attributes_end;
        let mut position = element.name.end;
        loop {
            position += self.source[position..end]
                .iter()
                .position(|byte| !byte.is_ascii_whitespace())?;
            let start = position;
            position += self.source[position..end]
                .iter()
                .position(|byte| *byte == b'=')?;
            let attribute = self.source[start..position].trim_ascii_end();
            position += 1;
            position += self.source[position..end]
                .iter()
                .position(|byte| !byte.is_ascii_whitespace())?;
            let quote = self.source[position];
            let value_start = position + 1;
            let length = self.source[value_start..end]
                .iter()
                .position(|byte| *byte == quote)?;
            position = value_start + length + 1;
            if attribute == name.as_bytes() {
                return Some(value_start..value_start + length);
            }
        }
    }

    /// Reads the element text, element markup, or attribute at `path`.
    pub(super) fn read(&self, path: &str) -> Result<XmlValue, XmlEditError> {
        let target = Target::parse(path)?;
        let not_found = || XmlEditError::NotFound(path.to_owned());
        let element = self.element(&target.elements).ok_or_else(not_found)?;
        if let Some(attribute) = target.attribute {
            let span = self
                .attribute_value(element, attribute)
                .ok_or_else(not_found)?;
            return Ok(XmlValue::Text(unescape(&self.text(span))));
        }
        let node = &self.elements[element];
        if !node.children.is_empty() {
            let end = node.close.as_ref().map_or(node.open_end, |close| close.end);
            return Ok(XmlValue::Markup(self.text(node.name.start - 1..end)));
        }
        Ok(XmlValue::Text(
            node.close.as_ref().map_or_else(String::new, |close| {
                unescape(&self.text(node.open_end..close.start))
            }),
        ))
    }

    /// Returns the document text with every `(path, value)` applied.
    ///
    /// A missing final element is appended to its parent; a missing attribute is added.
    pub(super) fn set(&self, values: &[(&str, &str)]) -> Result<String, XmlEditError> {
        let mut edits = Vec::new();
        let mut appended = BTreeMap::<usize, String>::new();
        for (path, value) in values {
            let target = Target::parse(path)?;
            if let Some(attribute) = target.attribute {
                let element = self
                    .element(&target.elements)
                    .ok_or_else(|| XmlEditError::NotFound((*path).to_owned()))?;
                edits.push(self.attribute_edit(element, attribute, value));
            } else if let Some(element) = self.element(&target.elements) {
                edits.push(self.text_edit(element, value, path)?);
            } else {
                let (parent, name) = self.missing_parent(&target, path)?;
                let escaped = escape(value, false);
                appended
                    .entry(parent)
                    .or_default()
                    .push_str(&format!("<{name}>{escaped}</{name}>"));
            }
        }
        edits.extend(
            appended
                .into_iter()
                .map(|(parent, children)| self.append_edit(parent, children)),
        );
        let output = self.splice(edits)?;
        roxmltree::Document::parse(&output).map_err(|_| XmlEditError::Malformed)?;
        Ok(output)
    }

    fn attribute_edit(&self, element: usize, name: &str, value: &str) -> Edit {
        let escaped = escape(value, true);
        match self.attribute_value(element, name) {
            Some(span) => Edit {
                span,
                text: escaped,
            },
            None => {
                let position = self.elements[element].attributes_end;
                Edit {
                    span: position..position,
                    text: format!(" {name}=\"{escaped}\""),
                }
            }
        }
    }

    fn text_edit(&self, element: usize, value: &str, path: &str) -> Result<Edit, XmlEditError> {
        let node = &self.elements[element];
        if !node.children.is_empty() {
            return Err(XmlEditError::HasChildElements(path.to_owned()));
        }
        let escaped = escape(value, false);
        Ok(match &node.close {
            Some(close) => Edit {
                span: node.open_end..close.start,
                text: escaped,
            },
            None => self.expand_empty_element(element, &escaped),
        })
    }

    fn missing_parent<'p>(
        &self,
        target: &Target<'p>,
        path: &str,
    ) -> Result<(usize, &'p str), XmlEditError> {
        let not_found = || XmlEditError::NotFound(path.to_owned());
        let (last, parents) = target.elements.split_last().ok_or_else(not_found)?;
        if last.index.is_some_and(|index| index != 0) {
            return Err(not_found());
        }
        Ok((self.element(parents).ok_or_else(not_found)?, last.name))
    }

    fn append_edit(&self, parent: usize, children: String) -> Edit {
        match &self.elements[parent].close {
            Some(close) => Edit {
                span: close.start..close.start,
                text: children,
            },
            None => self.expand_empty_element(parent, &children),
        }
    }

    fn expand_empty_element(&self, element: usize, content: &str) -> Edit {
        let node = &self.elements[element];
        Edit {
            span: node.attributes_end..node.open_end,
            text: format!(">{content}</{}>", self.text(node.name.clone())),
        }
    }

    fn splice(&self, mut edits: Vec<Edit>) -> Result<String, XmlEditError> {
        edits.sort_by_key(|edit| edit.span.start);
        let mut output = Vec::with_capacity(self.end);
        let mut position = 0;
        for edit in edits {
            if edit.span.start < position {
                return Err(XmlEditError::Overlap);
            }
            output.extend_from_slice(&self.source[position..edit.span.start]);
            output.extend_from_slice(edit.text.as_bytes());
            position = edit.span.end;
        }
        output.extend_from_slice(&self.source[position..self.end]);
        Ok(String::from_utf8_lossy(&output).into_owned())
    }
}

struct Edit {
    span: Range<usize>,
    text: String,
}

struct Segment<'a> {
    name: &'a str,
    index: Option<usize>,
}

/// A parsed XML path. Paths start below the root element, such as `Part0/Name` or `Part0/@id`.
struct Target<'a> {
    elements: Vec<Segment<'a>>,
    attribute: Option<&'a str>,
}

impl<'a> Target<'a> {
    fn parse(path: &'a str) -> Result<Self, XmlEditError> {
        let invalid = || XmlEditError::InvalidPath(path.to_owned());
        let mut parts = path.split('/').collect::<Vec<_>>();
        let attribute = parts.last().and_then(|last| last.strip_prefix('@'));
        if let Some(attribute) = attribute {
            parts.pop();
            if !is_name(attribute) {
                return Err(invalid());
            }
        }
        let elements = parts
            .into_iter()
            .map(|part| parse_segment(part).ok_or_else(invalid))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            elements,
            attribute,
        })
    }
}

fn parse_segment(part: &str) -> Option<Segment<'_>> {
    let (name, index) = match part.split_once('[') {
        Some((name, index)) => (name, Some(index.strip_suffix(']')?.parse().ok()?)),
        None => (part, None),
    };
    is_name(name).then_some(Segment { name, index })
}

fn is_name(name: &str) -> bool {
    fn start(c: char) -> bool {
        matches!(c, ':' | '_' | 'A'..='Z' | 'a'..='z' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}' | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
    }
    let mut chars = name.chars();
    chars.next().is_some_and(start) && chars.all(|c| start(c) || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}'))
}

fn escape(text: &str, attribute: bool) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' if attribute => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn unescape(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        output.push_str(&rest[..start]);
        rest = &rest[start..];
        let decoded = rest
            .find(';')
            .and_then(|end| decode_entity(&rest[1..end]).map(|character| (character, end)));
        match decoded {
            Some((character, end)) => {
                output.push(character);
                rest = &rest[end + 1..];
            }
            None => {
                output.push('&');
                rest = &rest[1..];
            }
        }
    }
    output.push_str(rest);
    output
}

fn decode_entity(entity: &str) -> Option<char> {
    match entity {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => {
            let number = entity.strip_prefix('#')?;
            let code = match number.strip_prefix('x') {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => number.parse().ok()?,
            };
            char::from_u32(code)
        }
    }
}

struct Parser<'a> {
    source: &'a [u8],
    position: usize,
    elements: Vec<Element>,
}

impl<'a> Parser<'a> {
    fn document(mut self) -> Option<Document<'a>> {
        self.skip_prolog()?;
        let mut open = Vec::<usize>::new();
        loop {
            self.position += self.source[self.position..]
                .iter()
                .position(|byte| *byte == b'<')?;
            if self.skip_markup()? {
                continue;
            }
            if self.source.get(self.position + 1) == Some(&b'/') {
                self.close_tag(open.pop()?)?;
            } else {
                let (element, self_closing) = self.open_tag()?;
                if let Some(parent) = open.last() {
                    self.elements[*parent].children.push(element);
                }
                if !self_closing {
                    open.push(element);
                }
            }
            if open.is_empty() {
                let end = self.position;
                return Some(Document {
                    source: self.source,
                    elements: self.elements,
                    end,
                });
            }
        }
    }

    fn starts_with(&self, prefix: &[u8]) -> bool {
        self.source[self.position..].starts_with(prefix)
    }

    fn skip_past(&mut self, terminator: &[u8]) -> Option<()> {
        let length = self.source[self.position..]
            .windows(terminator.len())
            .position(|window| window == terminator)?;
        self.position += length + terminator.len();
        Some(())
    }

    fn skip_prolog(&mut self) -> Option<()> {
        if self.starts_with(b"\xef\xbb\xbf") {
            self.position += 3;
        }
        loop {
            self.position += self.source[self.position..]
                .iter()
                .position(|byte| !byte.is_ascii_whitespace())?;
            if self.source[self.position] != b'<' {
                return None;
            }
            if !self.skip_markup()? {
                return Some(());
            }
        }
    }

    /// Skips a comment, CDATA section, processing instruction, or DOCTYPE at the cursor.
    fn skip_markup(&mut self) -> Option<bool> {
        let terminator: &[u8] = if self.starts_with(b"<!--") {
            b"-->"
        } else if self.starts_with(b"<![CDATA[") {
            b"]]>"
        } else if self.starts_with(b"<?") {
            b"?>"
        } else if self.starts_with(b"<!") {
            b">"
        } else {
            return Some(false);
        };
        self.skip_past(terminator)?;
        Some(true)
    }

    fn name(&mut self) -> Option<Range<usize>> {
        let start = self.position;
        let length = self.source[start..]
            .iter()
            .position(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>'))?;
        self.position += length;
        (length > 0).then_some(start..start + length)
    }

    fn open_tag(&mut self) -> Option<(usize, bool)> {
        self.position += 1;
        let name = self.name()?;
        let mut quote = None;
        loop {
            let byte = *self.source.get(self.position)?;
            match quote {
                Some(open) if byte == open => quote = None,
                Some(_) => {}
                None if byte == b'"' || byte == b'\'' => quote = Some(byte),
                None if byte == b'>' => break,
                None if byte == b'<' => return None,
                None => {}
            }
            self.position += 1;
        }
        let self_closing = self.source[self.position - 1] == b'/';
        let attributes_end = self.position - usize::from(self_closing);
        self.position += 1;
        self.elements.push(Element {
            name,
            children: Vec::new(),
            attributes_end,
            open_end: self.position,
            close: None,
        });
        Some((self.elements.len() - 1, self_closing))
    }

    fn close_tag(&mut self, element: usize) -> Option<()> {
        let start = self.position;
        self.position += 2;
        let name = self.name()?;
        if self.source[name] != self.source[self.elements[element].name.clone()] {
            return None;
        }
        self.skip_past(b">")?;
        self.elements[element].close = Some(start..self.position);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str = "<?xml version=\"1.0\"?>\n<Multi a=\"1\"><!-- <x> --><Part0><Name>A &amp; B</Name><Empty/></Part0><Part0 id='2'/></Multi>tail";

    #[test]
    fn reads_elements_attributes_and_repeated_elements() {
        let document = Document::parse(SOURCE.as_bytes()).unwrap();

        assert_eq!(document.root_name(), "Multi");
        assert_eq!(&SOURCE[document.end()..], "tail");
        assert_eq!(
            document.read("Part0/Name").unwrap(),
            XmlValue::Text("A & B".into())
        );
        assert_eq!(
            document.read("Part0[1]/@id").unwrap(),
            XmlValue::Text("2".into())
        );
        assert_eq!(document.read("@a").unwrap(), XmlValue::Text("1".into()));
        assert_eq!(
            document.read("Part0").unwrap(),
            XmlValue::Markup("<Part0><Name>A &amp; B</Name><Empty/></Part0>".into())
        );
    }

    #[test]
    fn edits_splice_only_the_addressed_text() {
        let document = Document::parse(SOURCE.as_bytes()).unwrap();

        let edited = document
            .set(&[
                ("Part0/Name", "<C>"),
                ("Part0/Empty", "x"),
                ("Part0/New1", "1"),
                ("Part0/New2", "2"),
                ("Part0[1]/Added", "y"),
                ("Part0[1]/@id", "3"),
                ("@b", "\"q\""),
            ])
            .unwrap();

        assert_eq!(
            edited,
            "<?xml version=\"1.0\"?>\n<Multi a=\"1\" b=\"&quot;q&quot;\"><!-- <x> --><Part0><Name>&lt;C&gt;</Name><Empty>x</Empty><New1>1</New1><New2>2</New2></Part0><Part0 id='3'><Added>y</Added></Part0></Multi>"
        );
    }

    #[test]
    fn rejects_unaddressable_edits() {
        let document = Document::parse(SOURCE.as_bytes()).unwrap();

        assert!(matches!(
            document.set(&[("Part0", "x")]),
            Err(XmlEditError::HasChildElements(_))
        ));
        assert!(matches!(
            document.set(&[("Missing/Name", "x")]),
            Err(XmlEditError::NotFound(_))
        ));
        assert!(matches!(
            document.set(&[("Part0/Name", "x"), ("Part0[0]/Name", "y")]),
            Err(XmlEditError::Overlap)
        ));
        for path in ["Part0/123", "Part0/@123", "Part0/a?b"] {
            assert!(matches!(
                document.set(&[(path, "x")]),
                Err(XmlEditError::InvalidPath(_))
            ));
        }
        assert!(matches!(
            document.set(&[("Part0/Name", "\0")]),
            Err(XmlEditError::Malformed)
        ));
        assert!(Document::parse(b"<a><b></a></b>").is_none());
    }
}
