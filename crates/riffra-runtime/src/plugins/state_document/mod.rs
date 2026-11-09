//! Structured access to the JUCE formats inside a VST3 plugin state.
//!
//! The host stores a plugin's component state as `VC2!` + `<VST3PluginState>` XML with a JUCE
//! base64 `<IComponent>`. JUCE plugins fill that component with a ValueTree or an XML blob,
//! optionally wrapped in an fxb bank chunk. Everything outside the addressed value is carried over
//! byte for byte.

mod value_tree;
mod xml;

use crate::api::output::{
    PluginStateContainer, PluginStateContent, PluginStateEncodedValue, PluginStateInspection,
    PluginStatePropertySummary, PluginStateReadResult, PluginStateTree, PluginStateTreeSummary,
    PluginStateValue,
};
use crate::api::params::PluginStatePatchOperation;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use riffra_control::{ErrorCode, ProtocolError};
use std::ops::Range;
use xml::{Document, XmlEditError, XmlValue};

const XML_BINARY_MAGIC: &[u8] = b"VC2!";
const XML_BINARY_HEADER_SIZE: usize = 8;
const COMPONENT_OPEN: &str = "<IComponent>";
const COMPONENT_CLOSE: &str = "</IComponent>";
const FXB_MAGIC: &[u8] = b"CcnK";
const FXB_OPAQUE_BANK: &[u8] = b"FBCh";
const FXB_SEARCH_LIMIT: usize = 64;
const FXB_BYTE_SIZE_OFFSET: usize = 4;
const FXB_KIND_OFFSET: usize = 8;
const FXB_CHUNK_SIZE_OFFSET: usize = 156;
const FXB_HEADER_SIZE: usize = 160;
const MAX_INLINE_TEXT_LENGTH: usize = 256;

/// Failure to address or change a value inside a plugin state.
#[derive(Debug, thiserror::Error)]
pub(crate) enum PluginStateDocumentError {
    #[error("plugin has not captured any state data")]
    Empty,
    #[error("plugin state does not use the JUCE host format")]
    UnsupportedHostFormat,
    #[error("plugin state component is not a readable JUCE format")]
    OpaqueComponent,
    #[error("plugin state path {0:?} is invalid")]
    InvalidPath(String),
    #[error("plugin state path {0:?} was not found")]
    NotFound(String),
    #[error("plugin state value at {0:?} cannot take the supplied type")]
    TypeMismatch(String),
    #[error("plugin state value at {0:?} does not contain XML")]
    NotXml(String),
    #[error("plugin state value at {0:?} is not a tree")]
    NotTree(String),
    #[error("replacement for {0:?} is not well-formed XML")]
    MalformedXml(String),
    #[error(transparent)]
    Xml(#[from] XmlEditError),
}

impl From<PluginStateDocumentError> for ProtocolError {
    fn from(error: PluginStateDocumentError) -> Self {
        let code = match error {
            PluginStateDocumentError::Empty
            | PluginStateDocumentError::UnsupportedHostFormat
            | PluginStateDocumentError::OpaqueComponent => ErrorCode::CommandFailed,
            _ => ErrorCode::InvalidRequest,
        };
        Self::new(code, error.to_string())
    }
}

type Result<T> = std::result::Result<T, PluginStateDocumentError>;

/// Describes the structure of `state_data` without returning large values.
pub(crate) fn inspect(state_data: Option<&str>) -> Result<PluginStateInspection> {
    let document = StateDocument::parse(state_data)?;
    Ok(PluginStateInspection {
        container: if document.fxb.is_some() {
            PluginStateContainer::Fxb
        } else {
            PluginStateContainer::Component
        },
        content: match &document.body {
            Body::Tree(tree) => PluginStateContent::ValueTree {
                tree: summarize_tree(tree),
            },
            Body::Data(value) => content_of(value),
        },
    })
}

/// Reads the tree, property, or XML value at `path`.
pub(crate) fn read(
    state_data: Option<&str>,
    path: &str,
    xml_path: Option<&str>,
) -> Result<PluginStateReadResult> {
    let mut document = StateDocument::parse(state_data)?;
    document.visit(path, |target| match (target, xml_path) {
        (Target::Node(_), Some(_)) => Err(PluginStateDocumentError::NotXml(path.to_owned())),
        (Target::Node(tree), None) => Ok(PluginStateReadResult::Tree { tree: tree.clone() }),
        (Target::Value(value), Some(xml_path)) => read_xml_value(value, path, xml_path),
        (Target::Value(value), None) => Ok(read_value(value)),
    })
}

/// Applies `operations` in order and returns the re-encoded state data.
pub(crate) fn patch(
    state_data: Option<&str>,
    operations: &[PluginStatePatchOperation],
) -> Result<String> {
    let mut document = StateDocument::parse(state_data)?;
    if matches!(&document.body, Body::Data(value) if embedded_xml_of(value).is_none()) {
        return Err(PluginStateDocumentError::OpaqueComponent);
    }
    for operation in operations {
        match operation {
            PluginStatePatchOperation::Set { path, value } => {
                document.visit(path, |target| assign(target, value.clone(), path))?
            }
            PluginStatePatchOperation::SetTree { path, tree } => {
                document.visit(path, |target| replace_tree(target, tree, path))?
            }
            PluginStatePatchOperation::SetXml { path, values } => {
                let values = values
                    .iter()
                    .map(|value| (value.xml_path.as_str(), value.value.as_str()))
                    .collect::<Vec<_>>();
                document.visit(path, |target| set_xml_values(target, &values, path))?
            }
        }
    }
    Ok(document.encode())
}

enum Body {
    Tree(PluginStateTree),
    Data(PluginStateValue),
}

/// The addressed item of a state document.
enum Target<'a> {
    Node(&'a mut PluginStateTree),
    Value(&'a mut PluginStateValue),
}

struct StateDocument {
    host: Vec<u8>,
    component_text: Range<usize>,
    component: Vec<u8>,
    fxb: Option<usize>,
    body_range: Range<usize>,
    body: Body,
}

impl StateDocument {
    fn parse(state_data: Option<&str>) -> Result<Self> {
        let unsupported = || PluginStateDocumentError::UnsupportedHostFormat;
        let encoded = state_data
            .filter(|data| !data.is_empty())
            .ok_or(PluginStateDocumentError::Empty)?;
        let host = STANDARD.decode(encoded).map_err(|_| unsupported())?;
        let host_xml = xml_binary_range(&host).ok_or_else(unsupported)?;
        let text = std::str::from_utf8(&host[host_xml.clone()]).map_err(|_| unsupported())?;
        let open = text.find(COMPONENT_OPEN).ok_or_else(unsupported)? + COMPONENT_OPEN.len();
        let close = open + text[open..].find(COMPONENT_CLOSE).ok_or_else(unsupported)?;
        let component = value_tree::decode_base64(&text[open..close]).ok_or_else(unsupported)?;
        let component_text = host_xml.start + open..host_xml.start + close;

        let fxb = find_fxb(&component);
        let start = fxb.map_or(0, |offset| offset + FXB_HEADER_SIZE);
        let chunk = &component[start..];
        let tree = (!chunk.starts_with(XML_BINARY_MAGIC))
            .then(|| value_tree::read_tree(chunk))
            .flatten();
        let (body, body_range) = match tree {
            Some((tree, used)) => (Body::Tree(tree), start..start + used),
            None => (
                Body::Data(PluginStateValue::Encoded(PluginStateEncodedValue::Binary {
                    base64: component[start..].to_vec(),
                })),
                start..component.len(),
            ),
        };
        Ok(Self {
            host,
            component_text,
            component,
            fxb,
            body_range,
            body,
        })
    }

    fn visit<R>(&mut self, path: &str, action: impl FnOnce(Target<'_>) -> Result<R>) -> Result<R> {
        let segments = parse_path(path)?;
        match &mut self.body {
            Body::Tree(tree) => visit_tree(tree, &segments, path, action),
            Body::Data(value) if segments.is_empty() => action(Target::Value(value)),
            Body::Data(_) => Err(PluginStateDocumentError::NotFound(path.to_owned())),
        }
    }

    fn encode(self) -> String {
        let body = match &self.body {
            Body::Tree(tree) => {
                let mut encoded = Vec::new();
                value_tree::write_tree(tree, &mut encoded);
                encoded
            }
            Body::Data(PluginStateValue::Encoded(PluginStateEncodedValue::Binary { base64 })) => {
                base64.clone()
            }
            Body::Data(_) => unreachable!("a state body is a tree or binary data"),
        };
        let growth = body.len() as i64 - self.body_range.len() as i64;
        let mut component = self.component;
        component.splice(self.body_range, body);
        if let Some(offset) = self.fxb {
            adjust_be_u32(&mut component, offset + FXB_BYTE_SIZE_OFFSET, growth);
            adjust_be_u32(&mut component, offset + FXB_CHUNK_SIZE_OFFSET, growth);
        }

        let text = value_tree::encode_base64(&component);
        let growth = text.len() as i64 - self.component_text.len() as i64;
        let mut host = self.host;
        host.splice(self.component_text, text.into_bytes());
        let size = read_le_u32(&host, 4).expect("host header was validated") as i64 + growth;
        host[4..XML_BINARY_HEADER_SIZE].copy_from_slice(&(size as u32).to_le_bytes());
        STANDARD.encode(host)
    }
}

fn find_fxb(component: &[u8]) -> Option<usize> {
    let window = &component[..component.len().min(FXB_SEARCH_LIMIT)];
    let offset = window
        .windows(FXB_MAGIC.len())
        .position(|bytes| bytes == FXB_MAGIC)?;
    let is_opaque_bank = component
        .get(offset + FXB_KIND_OFFSET..offset + FXB_KIND_OFFSET + FXB_OPAQUE_BANK.len())
        == Some(FXB_OPAQUE_BANK);
    (is_opaque_bank && component.len() >= offset + FXB_HEADER_SIZE).then_some(offset)
}

fn adjust_be_u32(bytes: &mut [u8], offset: usize, growth: i64) {
    let slot = &mut bytes[offset..offset + 4];
    let current = u32::from_be_bytes((&*slot).try_into().expect("slot is four bytes"));
    slot.copy_from_slice(&((i64::from(current) + growth) as u32).to_be_bytes());
}

fn read_le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn xml_binary_range(bytes: &[u8]) -> Option<Range<usize>> {
    if !bytes.starts_with(XML_BINARY_MAGIC) {
        return None;
    }
    let size = usize::try_from(read_le_u32(bytes, 4)?).ok()?;
    let end = XML_BINARY_HEADER_SIZE.checked_add(size)?;
    (end <= bytes.len()).then_some(XML_BINARY_HEADER_SIZE..end)
}

struct Segment<'a> {
    name: &'a str,
    index: Option<usize>,
}

fn parse_path(path: &str) -> Result<Vec<Segment<'_>>> {
    if path.is_empty() {
        return Ok(Vec::new());
    }
    path.split('/')
        .map(|part| {
            let (name, index) = match part.split_once('[') {
                Some((name, index)) => (
                    name,
                    Some(index.strip_suffix(']').and_then(|index| index.parse().ok())),
                ),
                None => (part, None),
            };
            match index {
                Some(None) => Err(PluginStateDocumentError::InvalidPath(path.to_owned())),
                _ if name.is_empty() => Err(PluginStateDocumentError::InvalidPath(path.to_owned())),
                index => Ok(Segment {
                    name,
                    index: index.flatten(),
                }),
            }
        })
        .collect()
}

fn child_position(tree: &PluginStateTree, segment: &Segment<'_>) -> Option<usize> {
    tree.children
        .iter()
        .enumerate()
        .filter(|(_, child)| child.tree_type == segment.name)
        .nth(segment.index.unwrap_or(0))
        .map(|(position, _)| position)
}

fn property_position(tree: &PluginStateTree, segment: &Segment<'_>) -> Option<usize> {
    if segment.index.is_some() {
        return None;
    }
    tree.properties
        .iter()
        .position(|property| property.name == segment.name)
}

fn visit_tree<R>(
    tree: &mut PluginStateTree,
    segments: &[Segment<'_>],
    path: &str,
    action: impl FnOnce(Target<'_>) -> Result<R>,
) -> Result<R> {
    let not_found = || PluginStateDocumentError::NotFound(path.to_owned());
    let Some((segment, rest)) = segments.split_first() else {
        return action(Target::Node(tree));
    };
    if rest.is_empty()
        && let Some(position) = property_position(tree, segment)
    {
        return action(Target::Value(&mut tree.properties[position].value));
    }
    if let Some(position) = child_position(tree, segment) {
        return visit_tree(&mut tree.children[position], rest, path, action);
    }
    let position = property_position(tree, segment).ok_or_else(not_found)?;
    let value = &mut tree.properties[position].value;
    let PluginStateValue::Text(text) = value else {
        return Err(not_found());
    };
    let mut nested = value_tree::decode_tree(text).ok_or_else(not_found)?;
    let result = visit_tree(&mut nested, rest, path, action)?;
    *value = PluginStateValue::Text(value_tree::encode_tree(&nested));
    Ok(result)
}

fn value_bytes(value: &PluginStateValue) -> Option<&[u8]> {
    match value {
        PluginStateValue::Text(text) => Some(text.as_bytes()),
        PluginStateValue::Encoded(PluginStateEncodedValue::Binary { base64 }) => Some(base64),
        _ => None,
    }
}

struct EmbeddedXml {
    range: Range<usize>,
    root: String,
}

fn embedded_xml(bytes: &[u8]) -> Option<EmbeddedXml> {
    if let Some(range) = xml_binary_range(bytes) {
        let root = Document::parse(&bytes[range.clone()])?.root_name();
        return Some(EmbeddedXml { range, root });
    }
    if bytes.first() != Some(&b'<') {
        return None;
    }
    let document = Document::parse(bytes)?;
    Some(EmbeddedXml {
        range: 0..document.end(),
        root: document.root_name(),
    })
}

fn embedded_xml_of(value: &PluginStateValue) -> Option<EmbeddedXml> {
    value_bytes(value).and_then(embedded_xml)
}

fn embedded_xml_text<'a>(bytes: &'a [u8], xml: &EmbeddedXml) -> Option<&'a str> {
    std::str::from_utf8(&bytes[xml.range.clone()]).ok()
}

fn rewrite_embedded_xml(
    value: &mut PluginStateValue,
    path: &str,
    edit: impl FnOnce(&str) -> Result<String>,
) -> Result<()> {
    let not_xml = || PluginStateDocumentError::NotXml(path.to_owned());
    let bytes = value_bytes(value).ok_or_else(not_xml)?;
    let xml = embedded_xml(bytes).ok_or_else(not_xml)?;
    let updated = edit(embedded_xml_text(bytes, &xml).ok_or_else(not_xml)?)?;

    let mut rewritten = bytes[..xml.range.start].to_vec();
    rewritten.extend_from_slice(updated.as_bytes());
    rewritten.extend_from_slice(&bytes[xml.range.end..]);
    if rewritten.starts_with(XML_BINARY_MAGIC) {
        rewritten[4..XML_BINARY_HEADER_SIZE].copy_from_slice(&(updated.len() as u32).to_le_bytes());
    }
    *value = match value {
        PluginStateValue::Text(_) => {
            PluginStateValue::Text(String::from_utf8(rewritten).map_err(|_| not_xml())?)
        }
        _ => PluginStateValue::Encoded(PluginStateEncodedValue::Binary { base64: rewritten }),
    };
    Ok(())
}

fn assign(target: Target<'_>, next: PluginStateValue, path: &str) -> Result<()> {
    let mismatch = || PluginStateDocumentError::TypeMismatch(path.to_owned());
    let Target::Value(value) = target else {
        return Err(mismatch());
    };
    if let (Some(_), PluginStateValue::Text(text)) = (embedded_xml_of(value), &next) {
        if Document::parse(text.as_bytes()).is_none() {
            return Err(PluginStateDocumentError::MalformedXml(path.to_owned()));
        }
        return rewrite_embedded_xml(value, path, |_| Ok(text.clone()));
    }
    *value = coerce(value, next).ok_or_else(mismatch)?;
    Ok(())
}

/// Converts `next` to the JUCE type already stored, so a value keeps its declared type.
fn coerce(current: &PluginStateValue, next: PluginStateValue) -> Option<PluginStateValue> {
    use PluginStateEncodedValue as Encoded;
    use PluginStateValue as Value;
    match (current, next) {
        (Value::Bool(_), Value::Bool(value)) => Some(Value::Bool(value)),
        (Value::Bool(_), Value::Text(text)) => match text.as_str() {
            "1" | "true" => Some(Value::Bool(true)),
            "0" | "false" => Some(Value::Bool(false)),
            _ => None,
        },
        (Value::Integer(_), Value::Integer(value)) => Some(Value::Integer(value)),
        (Value::Integer(_), Value::Text(text)) => text.parse().ok().map(Value::Integer),
        (Value::Double(_), Value::Double(value)) => Some(Value::Double(value)),
        (Value::Double(_), Value::Integer(value)) => Some(Value::Double(f64::from(value))),
        (Value::Double(_), Value::Text(text)) => text.parse().ok().map(Value::Double),
        (Value::Text(_), Value::Text(text)) => Some(Value::Text(text)),
        (Value::Text(_), Value::Bool(value)) => Some(Value::Text(value.to_string())),
        (Value::Text(_), Value::Integer(value)) => Some(Value::Text(value.to_string())),
        (Value::Text(_), Value::Double(value)) => Some(Value::Text(value.to_string())),
        (Value::Encoded(Encoded::Int64 { .. }), Value::Integer(value)) => {
            Some(Value::Encoded(Encoded::Int64 {
                value: i64::from(value),
            }))
        }
        (Value::Encoded(Encoded::Int64 { .. }), Value::Text(text)) => text
            .parse()
            .ok()
            .map(|value| Value::Encoded(Encoded::Int64 { value })),
        (Value::Encoded(current), Value::Encoded(next))
            if std::mem::discriminant(current) == std::mem::discriminant(&next) =>
        {
            Some(Value::Encoded(next))
        }
        _ => None,
    }
}

fn replace_tree(target: Target<'_>, tree: &PluginStateTree, path: &str) -> Result<()> {
    match target {
        Target::Node(node) => *node = tree.clone(),
        Target::Value(PluginStateValue::Text(text)) if value_tree::decode_tree(text).is_some() => {
            *text = value_tree::encode_tree(tree);
        }
        Target::Value(_) => return Err(PluginStateDocumentError::NotTree(path.to_owned())),
    }
    Ok(())
}

fn set_xml_values(target: Target<'_>, values: &[(&str, &str)], path: &str) -> Result<()> {
    let not_xml = || PluginStateDocumentError::NotXml(path.to_owned());
    let Target::Value(value) = target else {
        return Err(not_xml());
    };
    rewrite_embedded_xml(value, path, |text| {
        Ok(Document::parse(text.as_bytes())
            .ok_or_else(not_xml)?
            .set(values)?)
    })
}

fn read_value(value: &PluginStateValue) -> PluginStateReadResult {
    if let Some(text) = value_bytes(value).and_then(|bytes| {
        let xml = embedded_xml(bytes)?;
        embedded_xml_text(bytes, &xml).map(str::to_owned)
    }) {
        return PluginStateReadResult::Xml { text };
    }
    if let PluginStateValue::Text(text) = value
        && let Some(tree) = value_tree::decode_tree(text)
    {
        return PluginStateReadResult::Tree { tree };
    }
    PluginStateReadResult::Value {
        value: value.clone(),
    }
}

fn read_xml_value(
    value: &PluginStateValue,
    path: &str,
    xml_path: &str,
) -> Result<PluginStateReadResult> {
    let not_xml = || PluginStateDocumentError::NotXml(path.to_owned());
    let bytes = value_bytes(value).ok_or_else(not_xml)?;
    let xml = embedded_xml(bytes).ok_or_else(not_xml)?;
    let document = Document::parse(&bytes[xml.range]).ok_or_else(not_xml)?;
    Ok(match document.read(xml_path)? {
        XmlValue::Text(text) => PluginStateReadResult::Value {
            value: PluginStateValue::Text(text),
        },
        XmlValue::Markup(text) => PluginStateReadResult::Xml { text },
    })
}

fn summarize_tree(tree: &PluginStateTree) -> PluginStateTreeSummary {
    PluginStateTreeSummary {
        tree_type: tree.tree_type.clone(),
        properties: tree
            .properties
            .iter()
            .map(|property| PluginStatePropertySummary {
                name: property.name.clone(),
                content: content_of(&property.value),
            })
            .collect(),
        children: tree.children.iter().map(summarize_tree).collect(),
    }
}

fn content_of(value: &PluginStateValue) -> PluginStateContent {
    if let Some(xml) = embedded_xml_of(value) {
        return PluginStateContent::Xml {
            root: xml.root,
            bytes: xml.range.len(),
        };
    }
    match value {
        PluginStateValue::Text(text) => {
            if let Some(tree) = value_tree::decode_tree(text) {
                return PluginStateContent::ValueTree {
                    tree: summarize_tree(&tree),
                };
            }
            if text.len() > MAX_INLINE_TEXT_LENGTH {
                return PluginStateContent::Data { bytes: text.len() };
            }
            PluginStateContent::Value {
                value: value.clone(),
            }
        }
        PluginStateValue::Encoded(
            PluginStateEncodedValue::Binary { base64 }
            | PluginStateEncodedValue::Opaque { base64, .. },
        ) => PluginStateContent::Data {
            bytes: base64.len(),
        },
        other => PluginStateContent::Value {
            value: other.clone(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::output::PluginStateProperty;
    use crate::api::params::PluginStateXmlValue;
    use serde_json::json;

    fn property(name: &str, value: PluginStateValue) -> PluginStateProperty {
        PluginStateProperty {
            name: name.into(),
            value,
        }
    }

    fn binary(bytes: Vec<u8>) -> PluginStateValue {
        PluginStateValue::Encoded(PluginStateEncodedValue::Binary { base64: bytes })
    }

    fn xml_binary(xml: &str) -> Vec<u8> {
        let mut bytes = XML_BINARY_MAGIC.to_vec();
        bytes.extend_from_slice(&(xml.len() as u32).to_le_bytes());
        bytes.extend_from_slice(xml.as_bytes());
        bytes.push(0);
        bytes
    }

    fn host_state(component: &[u8]) -> String {
        let xml = format!(
            "<VST3PluginState><IComponent>{}</IComponent><IEditController>1.A</IEditController></VST3PluginState>",
            value_tree::encode_base64(component)
        );
        STANDARD.encode(xml_binary(&xml))
    }

    fn fxb_component(chunk: &[u8]) -> Vec<u8> {
        let mut component = b"VstW\0\0\0\x08\0\0\0\x01\0\0\0\0".to_vec();
        let ccnk = component.len();
        component.extend_from_slice(FXB_MAGIC);
        component.extend_from_slice(&((FXB_HEADER_SIZE - 8 + chunk.len()) as u32).to_be_bytes());
        component.extend_from_slice(FXB_OPAQUE_BANK);
        component.resize(ccnk + FXB_CHUNK_SIZE_OFFSET, 7);
        component.extend_from_slice(&(chunk.len() as u32).to_be_bytes());
        component.extend_from_slice(chunk);
        component
    }

    fn plugin_tree() -> PluginStateTree {
        let nested = PluginStateTree {
            tree_type: "AmpState".into(),
            properties: vec![property("Gain", PluginStateValue::Double(4.0))],
            children: Vec::new(),
        };
        PluginStateTree {
            tree_type: "State".into(),
            properties: vec![
                property(
                    "Plugin",
                    binary(xml_binary("<Multi><Part0><Level>1</Level></Part0></Multi>")),
                ),
                property(
                    "Preset",
                    binary(b"<Program a=\"1\"><X/></Program>\0\x9f".to_vec()),
                ),
                property(
                    "Amp",
                    PluginStateValue::Text(value_tree::encode_tree(&nested)),
                ),
                property("Gain", PluginStateValue::Double(0.5)),
                property("Label", PluginStateValue::Text("one".into())),
            ],
            children: vec![PluginStateTree {
                tree_type: "Slot".into(),
                properties: vec![property("On", PluginStateValue::Bool(false))],
                children: Vec::new(),
            }],
        }
    }

    fn component_with_tree(tree: &PluginStateTree, trailer: &[u8]) -> Vec<u8> {
        let mut chunk = Vec::new();
        value_tree::write_tree(tree, &mut chunk);
        chunk.extend_from_slice(trailer);
        fxb_component(&chunk)
    }

    fn decoded_component(state: &str) -> Vec<u8> {
        let document = StateDocument::parse(Some(state)).unwrap();
        assert_eq!(
            read_le_u32(&document.host, 4).unwrap() as usize + XML_BINARY_HEADER_SIZE + 1,
            document.host.len()
        );
        document.component
    }

    fn operations(value: serde_json::Value) -> Vec<PluginStatePatchOperation> {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn encoding_without_edits_reproduces_the_state() {
        let state = host_state(&component_with_tree(&plugin_tree(), b"JUCEPrivateData\x01"));

        let encoded = StateDocument::parse(Some(&state)).unwrap().encode();

        assert_eq!(encoded, state);
    }

    #[test]
    fn patch_updates_values_and_container_sizes() {
        let trailer = b"JUCEPrivateData\x01";
        let state = host_state(&component_with_tree(&plugin_tree(), trailer));

        let patched = patch(
            Some(&state),
            &operations(json!([
                {"op":"setXml","path":"Plugin","values":[
                    {"xmlPath":"Part0/Path","value":"a & b"},
                    {"xmlPath":"Part0/Level","value":"2"}
                ]},
                {"op":"setXml","path":"Preset","values":[{"xmlPath":"@a","value":"22"}]},
                {"op":"set","path":"Gain","value":"0.75"},
                {"op":"set","path":"Label","value":3},
                {"op":"set","path":"Amp/Gain","value":6},
                {"op":"set","path":"Slot/On","value":true}
            ])),
        )
        .unwrap();

        let component = decoded_component(&patched);
        let offset = find_fxb(&component).unwrap();
        let size = |field: usize| {
            u32::from_be_bytes(
                component[offset + field..offset + field + 4]
                    .try_into()
                    .unwrap(),
            ) as usize
        };
        assert_eq!(size(FXB_BYTE_SIZE_OFFSET), component.len() - offset - 8);
        assert_eq!(
            size(FXB_CHUNK_SIZE_OFFSET),
            component.len() - offset - FXB_HEADER_SIZE
        );
        assert!(component.ends_with(trailer));
        let (tree, _) = value_tree::read_tree(&component[offset + FXB_HEADER_SIZE..]).unwrap();
        assert_eq!(
            tree.properties[0].value,
            binary(xml_binary(
                "<Multi><Part0><Level>2</Level><Path>a &amp; b</Path></Part0></Multi>"
            ))
        );
        assert_eq!(
            tree.properties[1].value,
            binary(b"<Program a=\"22\"><X/></Program>\0\x9f".to_vec())
        );
        assert_eq!(tree.properties[3].value, PluginStateValue::Double(0.75));
        assert_eq!(tree.properties[4].value, PluginStateValue::Text("3".into()));
        assert_eq!(
            tree.children[0].properties[0].value,
            PluginStateValue::Bool(true)
        );
        let PluginStateReadResult::Tree { tree: amp } = read(Some(&patched), "Amp", None).unwrap()
        else {
            panic!("Amp holds a nested tree");
        };
        assert_eq!(amp.properties[0].value, PluginStateValue::Double(6.0));
    }

    #[test]
    fn set_tree_and_whole_xml_replace_their_target() {
        let state = host_state(&component_with_tree(&plugin_tree(), b""));

        let patched = patch(
            Some(&state),
            &operations(json!([
                {"op":"set","path":"Plugin","value":"<Multi><Part1/></Multi>"},
                {"op":"setTree","path":"Slot","tree":{
                    "type":"Slot","properties":[{"name":"Id","value":"Crusher"}],"children":[]
                }},
                {"op":"setTree","path":"Amp","tree":{
                    "type":"TubeState","properties":[{"name":"Drive","value":1.5}],"children":[]
                }}
            ])),
        )
        .unwrap();

        assert!(matches!(
            read(Some(&patched), "Plugin", None).unwrap(),
            PluginStateReadResult::Xml { text } if text == "<Multi><Part1/></Multi>"
        ));
        assert!(matches!(
            read(Some(&patched), "Slot/Id", None).unwrap(),
            PluginStateReadResult::Value { value: PluginStateValue::Text(id) } if id == "Crusher"
        ));
        assert!(matches!(
            read(Some(&patched), "Amp/Drive", None).unwrap(),
            PluginStateReadResult::Value { value: PluginStateValue::Double(drive) } if drive == 1.5
        ));
    }

    #[test]
    fn inspect_summarizes_and_read_addresses_xml() {
        let state = host_state(&component_with_tree(&plugin_tree(), b""));

        let inspection = inspect(Some(&state)).unwrap();
        let value = serde_json::to_value(inspection).unwrap();

        assert_eq!(value["container"], "fxb");
        assert_eq!(value["content"]["tree"]["type"], "State");
        let properties = &value["content"]["tree"]["properties"];
        assert_eq!(properties[0]["content"]["kind"], "xml");
        assert_eq!(properties[0]["content"]["root"], "Multi");
        assert_eq!(properties[2]["content"]["kind"], "valueTree");
        assert_eq!(properties[3]["content"]["value"], 0.5);
        assert!(matches!(
            read(Some(&state), "Plugin", Some("Part0/Level")).unwrap(),
            PluginStateReadResult::Value { value: PluginStateValue::Text(level) } if level == "1"
        ));
    }

    #[test]
    fn xml_components_accept_edits_at_the_root_path() {
        let mut component = xml_binary("<Patch><Cutoff>1</Cutoff></Patch>");
        component.extend_from_slice(b"trailer");
        let state = host_state(&component);

        let patched = patch(
            Some(&state),
            &[PluginStatePatchOperation::SetXml {
                path: String::new(),
                values: vec![PluginStateXmlValue {
                    xml_path: "Cutoff".into(),
                    value: "2".into(),
                }],
            }],
        )
        .unwrap();

        let component = decoded_component(&patched);
        assert!(component.ends_with(b"\0trailer"));
        assert!(matches!(
            read(Some(&patched), "", Some("Cutoff")).unwrap(),
            PluginStateReadResult::Value { value: PluginStateValue::Text(cutoff) } if cutoff == "2"
        ));
    }

    #[test]
    fn rejects_unaddressable_or_unknown_states() {
        let state = host_state(&component_with_tree(&plugin_tree(), b""));
        let opaque = host_state(b"\xde\xad\xbe\xef");

        for (operations, expected) in [
            (
                json!([{"op":"set","path":"Missing","value":1}]),
                "not found",
            ),
            (
                json!([{"op":"set","path":"Gain","value":"loud"}]),
                "supplied type",
            ),
            (
                json!([{"op":"set","path":"Plugin","value":"<Multi>"}]),
                "well-formed",
            ),
            (
                json!([{"op":"setXml","path":"Gain","values":[{"xmlPath":"A","value":"1"}]}]),
                "does not contain xml",
            ),
        ] {
            let error = patch(Some(&state), &self::operations(operations)).unwrap_err();
            assert!(
                error.to_string().to_lowercase().contains(expected),
                "{error}"
            );
        }
        assert!(matches!(
            patch(Some(&opaque), &[]),
            Err(PluginStateDocumentError::OpaqueComponent)
        ));
        assert!(matches!(
            serde_json::to_value(inspect(Some(&opaque)).unwrap()).unwrap()["content"]["kind"]
                .as_str(),
            Some("data")
        ));
        assert!(matches!(
            inspect(None),
            Err(PluginStateDocumentError::Empty)
        ));
        assert!(matches!(
            inspect(Some("AAAA")),
            Err(PluginStateDocumentError::UnsupportedHostFormat)
        ));
    }
}
