//! JUCE state container used by the VoiSona instrument.
//!
//! Values not edited by the adapter are kept as opaque bytes so plugin-specific
//! data survives a score update.

use base64::{Engine, engine::general_purpose::STANDARD};

const ALPHABET: &[u8; 64] = b".ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+";
const MAX_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Node {
    pub(super) name: String,
    pub(super) properties: Vec<(String, Vec<u8>)>,
    pub(super) children: Vec<Node>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(count)
            .filter(|end| *end <= self.bytes.len())
            .ok_or("truncated VoiSona state")?;
        let bytes = &self.bytes[self.position..end];
        self.position = end;
        Ok(bytes)
    }
    fn string(&mut self) -> Result<String, String> {
        let rest = &self.bytes[self.position..];
        let len = rest
            .iter()
            .position(|byte| *byte == 0)
            .ok_or("unterminated VoiSona state string")?;
        let bytes = self.take(len + 1)?;
        Ok(std::str::from_utf8(&bytes[..len])
            .map_err(|_| "invalid VoiSona state string")?
            .to_owned())
    }
    fn count(&mut self) -> Result<usize, String> {
        let width = self.take(1)?[0] as usize;
        if width > 4 {
            return Err("invalid VoiSona state length".into());
        }
        let mut value = 0usize;
        for (index, byte) in self.take(width)?.iter().enumerate() {
            value |= usize::from(*byte) << (index * 8);
        }
        Ok(value)
    }
    fn node(&mut self, depth: usize) -> Result<Node, String> {
        if depth > MAX_DEPTH {
            return Err("VoiSona state nesting is too deep".into());
        }
        let name = self.string()?;
        let count = self.count()?;
        if count > self.bytes.len().saturating_sub(self.position) {
            return Err("invalid VoiSona property count".into());
        }
        let mut properties = Vec::with_capacity(count);
        for _ in 0..count {
            let name = self.string()?;
            let len = self.count()?;
            properties.push((name, self.take(len)?.to_vec()));
        }
        let count = self.count()?;
        if count > self.bytes.len().saturating_sub(self.position) {
            return Err("invalid VoiSona child count".into());
        }
        let mut children = Vec::with_capacity(count);
        for _ in 0..count {
            children.push(self.node(depth + 1)?);
        }
        Ok(Node {
            name,
            properties,
            children,
        })
    }
}

fn write_count(value: usize, out: &mut Vec<u8>) -> Result<(), String> {
    let value = u32::try_from(value).map_err(|_| "VoiSona state is too large")?;
    let width = ((32 - value.leading_zeros()).div_ceil(8)).max(1) as usize;
    out.push(width as u8);
    out.extend_from_slice(&value.to_le_bytes()[..width]);
    Ok(())
}

fn write_node(node: &Node, out: &mut Vec<u8>) -> Result<(), String> {
    out.extend_from_slice(node.name.as_bytes());
    out.push(0);
    write_count(node.properties.len(), out)?;
    for (name, value) in &node.properties {
        out.extend_from_slice(name.as_bytes());
        out.push(0);
        write_count(value.len(), out)?;
        out.extend_from_slice(value);
    }
    write_count(node.children.len(), out)?;
    for child in &node.children {
        write_node(child, out)?;
    }
    Ok(())
}

pub(super) struct PluginState {
    prefix: Vec<u8>,
    suffix: Vec<u8>,
    tail: Vec<u8>,
    pub(super) root: Node,
}

impl PluginState {
    #[cfg(test)]
    pub(super) fn for_test(root: Node) -> Self {
        Self {
            prefix: b"<VST3PluginState><IComponent>".to_vec(),
            suffix: b"</IComponent></VST3PluginState>".to_vec(),
            tail: vec![0],
            root,
        }
    }
    pub(super) fn parse(encoded: &str) -> Result<Self, String> {
        let data = STANDARD
            .decode(encoded)
            .map_err(|_| "invalid plugin state base64")?;
        if data.len() < 9 || &data[..4] != b"VC2!" {
            return Err("unsupported VoiSona host state".into());
        }
        let xml_len = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
        if xml_len > data.len() - 8 {
            return Err("truncated VoiSona host state".into());
        }
        let xml = &data[8..8 + xml_len];
        let start_tag = b"<IComponent>";
        let end_tag = b"</IComponent>";
        let start = xml
            .windows(start_tag.len())
            .position(|p| p == start_tag)
            .ok_or("VoiSona component state is missing")?
            + start_tag.len();
        let end = xml[start..]
            .windows(end_tag.len())
            .position(|p| p == end_tag)
            .map(|p| p + start)
            .ok_or("invalid VoiSona component state")?;
        let component =
            std::str::from_utf8(&xml[start..end]).map_err(|_| "invalid VoiSona component state")?;
        let (length, digits) = component
            .split_once('.')
            .ok_or("invalid VoiSona component encoding")?;
        let length: usize = length
            .parse()
            .map_err(|_| "invalid VoiSona component length")?;
        if length > data.len().saturating_mul(2) || digits.len() != (length * 8).div_ceil(6) {
            return Err("invalid VoiSona component length".into());
        }
        let mut bytes = vec![0u8; length];
        for (index, digit) in digits.bytes().enumerate() {
            let value = ALPHABET
                .iter()
                .position(|candidate| *candidate == digit)
                .ok_or("invalid VoiSona component digit")? as u8;
            let bit = index * 6;
            if bit / 8 < length {
                bytes[bit / 8] |= value << (bit % 8);
            }
            if bit / 8 + 1 < length && bit % 8 > 2 {
                bytes[bit / 8 + 1] |= value >> (8 - bit % 8);
            }
        }
        let mut reader = Reader {
            bytes: &bytes,
            position: 0,
        };
        let root = reader.node(0)?;
        if root.name != "StateInformation" || reader.position != bytes.len() {
            return Err("unsupported VoiSona state tree".into());
        }
        Ok(Self {
            prefix: xml[..start].to_vec(),
            suffix: xml[end..].to_vec(),
            tail: data[8 + xml_len..].to_vec(),
            root,
        })
    }

    pub(super) fn encode(&self) -> Result<String, String> {
        let mut bytes = Vec::new();
        write_node(&self.root, &mut bytes)?;
        let mut component = format!("{}.", bytes.len());
        for bit in (0..bytes.len() * 8).step_by(6) {
            let mut value = bytes[bit / 8] >> (bit % 8);
            if bit % 8 > 2 && bit / 8 + 1 < bytes.len() {
                value |= bytes[bit / 8 + 1] << (8 - bit % 8);
            }
            component.push(ALPHABET[(value & 63) as usize] as char);
        }
        let xml_len = self.prefix.len() + component.len() + self.suffix.len();
        let mut result = b"VC2!".to_vec();
        result.extend_from_slice(
            &u32::try_from(xml_len)
                .map_err(|_| "VoiSona state is too large")?
                .to_le_bytes(),
        );
        result.extend_from_slice(&self.prefix);
        result.extend_from_slice(component.as_bytes());
        result.extend_from_slice(&self.suffix);
        result.extend_from_slice(&self.tail);
        Ok(STANDARD.encode(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_opaque_properties_and_host_state() {
        let original = PluginState {
            prefix: b"<VST3PluginState><IComponent>".to_vec(),
            suffix: b"</IComponent><IEditController>other</IEditController></VST3PluginState>"
                .to_vec(),
            tail: vec![0],
            root: Node {
                name: "StateInformation".into(),
                properties: vec![("Voice".into(), vec![8, 42, 0, 255])],
                children: vec![Node {
                    name: "Song".into(),
                    properties: vec![],
                    children: vec![],
                }],
            },
        };
        let encoded = original.encode().unwrap();
        let decoded = PluginState::parse(&encoded).unwrap();
        assert_eq!(decoded.root, original.root);
        assert_eq!(decoded.prefix, original.prefix);
        assert_eq!(decoded.suffix, original.suffix);
        assert_eq!(decoded.tail, original.tail);
        assert_eq!(decoded.encode().unwrap(), encoded);
        assert!(PluginState::parse("invalid").is_err());
    }
}
