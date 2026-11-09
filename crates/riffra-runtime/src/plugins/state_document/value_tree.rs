//! JUCE's binary encodings: `MemoryBlock` base64, compressed integers, `var`, and `ValueTree`.

use crate::api::output::{
    PluginStateEncodedValue, PluginStateProperty, PluginStateTree, PluginStateValue,
};

const BASE64_ALPHABET: &[u8; 64] =
    b".ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+";
const BASE64_INVALID: u8 = u8::MAX;
const BASE64_VALUES: [u8; 256] = base64_values();

const MARKER_INT: u8 = 1;
const MARKER_TRUE: u8 = 2;
const MARKER_FALSE: u8 = 3;
const MARKER_DOUBLE: u8 = 4;
const MARKER_STRING: u8 = 5;
const MARKER_INT64: u8 = 6;
const MARKER_BINARY: u8 = 8;

const MAX_TREE_DEPTH: usize = 128;

const fn base64_values() -> [u8; 256] {
    let mut values = [BASE64_INVALID; 256];
    let mut index = 0;
    while index < BASE64_ALPHABET.len() {
        values[BASE64_ALPHABET[index] as usize] = index as u8;
        index += 1;
    }
    values
}

fn base64_char_count(byte_count: usize) -> usize {
    (byte_count * 8).div_ceil(6)
}

/// Decodes JUCE's `MemoryBlock::toBase64Encoding` text, which is `<size>.<characters>`.
pub(super) fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let (size, characters) = text.split_once('.')?;
    let size = size.parse::<usize>().ok()?;
    if characters.len() != base64_char_count(size) {
        return None;
    }
    let mut decoded = vec![0_u8; size];
    for (index, character) in characters.bytes().enumerate() {
        let value = BASE64_VALUES[usize::from(character)];
        if value == BASE64_INVALID {
            return None;
        }
        let bit = index * 6;
        let (byte, shift) = (bit / 8, bit % 8);
        decoded[byte] |= value << shift;
        if shift > 2 && byte + 1 < size {
            decoded[byte + 1] |= value >> (8 - shift);
        }
    }
    Some(decoded)
}

/// Encodes bytes as JUCE's `MemoryBlock::toBase64Encoding` text.
pub(super) fn encode_base64(data: &[u8]) -> String {
    let count = base64_char_count(data.len());
    let mut encoded = data.len().to_string();
    encoded.reserve(count + 1);
    encoded.push('.');
    for index in 0..count {
        let bit = index * 6;
        let (byte, shift) = (bit / 8, bit % 8);
        let mut value = data[byte] >> shift;
        if shift > 2 && byte + 1 < data.len() {
            value |= data[byte + 1] << (8 - shift);
        }
        encoded.push(char::from(BASE64_ALPHABET[usize::from(value & 63)]));
    }
    encoded
}

/// Parses a `ValueTree` at the start of `data` and returns it with its encoded length.
pub(super) fn read_tree(data: &[u8]) -> Option<(PluginStateTree, usize)> {
    let mut reader = Reader { data, position: 0 };
    let tree = reader.tree(0)?;
    Some((tree, reader.position))
}

/// Decodes base64 text that holds exactly one `ValueTree`.
pub(super) fn decode_tree(text: &str) -> Option<PluginStateTree> {
    let data = decode_base64(text)?;
    read_tree(&data)
        .filter(|(_, used)| *used == data.len())
        .map(|(tree, _)| tree)
}

/// Encodes one `ValueTree` as base64 text.
pub(super) fn encode_tree(tree: &PluginStateTree) -> String {
    let mut data = Vec::new();
    write_tree(tree, &mut data);
    encode_base64(&data)
}

/// Appends the binary form of `tree` to `out`.
pub(super) fn write_tree(tree: &PluginStateTree, out: &mut Vec<u8>) {
    write_text(&tree.tree_type, out);
    write_count(tree.properties.len(), out);
    for property in &tree.properties {
        write_text(&property.name, out);
        write_value(&property.value, out);
    }
    write_count(tree.children.len(), out);
    for child in &tree.children {
        write_tree(child, out);
    }
}

fn write_text(text: &str, out: &mut Vec<u8>) {
    out.extend_from_slice(text.as_bytes());
    out.push(0);
}

fn write_count(count: usize, out: &mut Vec<u8>) {
    let magnitude = count.to_le_bytes();
    let width = magnitude
        .iter()
        .rposition(|byte| *byte != 0)
        .map_or(0, |index| index + 1);
    out.push(width as u8);
    out.extend_from_slice(&magnitude[..width]);
}

fn write_value(value: &PluginStateValue, out: &mut Vec<u8>) {
    let mut payload = Vec::new();
    match value {
        PluginStateValue::Bool(true) => payload.push(MARKER_TRUE),
        PluginStateValue::Bool(false) => payload.push(MARKER_FALSE),
        PluginStateValue::Integer(value) => {
            payload.push(MARKER_INT);
            payload.extend_from_slice(&value.to_le_bytes());
        }
        PluginStateValue::Double(value) => {
            payload.push(MARKER_DOUBLE);
            payload.extend_from_slice(&value.to_le_bytes());
        }
        PluginStateValue::Text(text) => {
            payload.push(MARKER_STRING);
            write_text(text, &mut payload);
        }
        PluginStateValue::Encoded(PluginStateEncodedValue::Void) => {}
        PluginStateValue::Encoded(PluginStateEncodedValue::Int64 { value }) => {
            payload.push(MARKER_INT64);
            payload.extend_from_slice(&value.to_le_bytes());
        }
        PluginStateValue::Encoded(PluginStateEncodedValue::Binary { base64 }) => {
            payload.push(MARKER_BINARY);
            payload.extend_from_slice(base64);
        }
        PluginStateValue::Encoded(PluginStateEncodedValue::Opaque { marker, base64 }) => {
            payload.push(*marker);
            payload.extend_from_slice(base64);
        }
    }
    write_count(payload.len(), out);
    out.extend_from_slice(&payload);
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(count)?;
        let bytes = self.data.get(self.position..end)?;
        self.position = end;
        Some(bytes)
    }

    fn count(&mut self) -> Option<usize> {
        let width = usize::from(*self.take(1)?.first()?);
        if width > 4 {
            return None;
        }
        let mut bytes = [0_u8; 4];
        bytes[..width].copy_from_slice(self.take(width)?);
        usize::try_from(u32::from_le_bytes(bytes)).ok()
    }

    fn text(&mut self) -> Option<&'a str> {
        let rest = self.data.get(self.position..)?;
        let end = rest.iter().position(|byte| *byte == 0)?;
        let text = std::str::from_utf8(&rest[..end]).ok()?;
        self.position += end + 1;
        Some(text)
    }

    fn value(&mut self) -> Option<PluginStateValue> {
        let size = self.count()?;
        let payload = self.take(size)?;
        let Some((marker, body)) = payload.split_first() else {
            return Some(PluginStateValue::Encoded(PluginStateEncodedValue::Void));
        };
        Some(decode_value(*marker, body))
    }

    fn tree(&mut self, depth: usize) -> Option<PluginStateTree> {
        if depth > MAX_TREE_DEPTH {
            return None;
        }
        let tree_type = self.text()?;
        if !is_identifier(tree_type) {
            return None;
        }
        let mut properties = Vec::new();
        for _ in 0..self.count()? {
            let name = self.text()?;
            if !is_identifier(name) {
                return None;
            }
            properties.push(PluginStateProperty {
                name: name.to_owned(),
                value: self.value()?,
            });
        }
        let mut children = Vec::new();
        for _ in 0..self.count()? {
            children.push(self.tree(depth + 1)?);
        }
        Some(PluginStateTree {
            tree_type: tree_type.to_owned(),
            properties,
            children,
        })
    }
}

fn is_identifier(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_graphic())
}

fn decode_value(marker: u8, body: &[u8]) -> PluginStateValue {
    let decoded = match marker {
        MARKER_INT => body
            .try_into()
            .ok()
            .map(i32::from_le_bytes)
            .map(PluginStateValue::Integer),
        MARKER_TRUE if body.is_empty() => Some(PluginStateValue::Bool(true)),
        MARKER_FALSE if body.is_empty() => Some(PluginStateValue::Bool(false)),
        MARKER_DOUBLE => body
            .try_into()
            .ok()
            .map(f64::from_le_bytes)
            .map(PluginStateValue::Double),
        MARKER_STRING => body
            .split_last()
            .filter(|(terminator, _)| **terminator == 0)
            .and_then(|(_, text)| std::str::from_utf8(text).ok())
            .map(|text| PluginStateValue::Text(text.to_owned())),
        MARKER_INT64 => body
            .try_into()
            .ok()
            .map(i64::from_le_bytes)
            .map(|value| PluginStateValue::Encoded(PluginStateEncodedValue::Int64 { value })),
        MARKER_BINARY => Some(PluginStateValue::Encoded(PluginStateEncodedValue::Binary {
            base64: body.to_vec(),
        })),
        _ => None,
    };
    decoded.unwrap_or_else(|| {
        PluginStateValue::Encoded(PluginStateEncodedValue::Opaque {
            marker,
            base64: body.to_vec(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_round_trips_every_trailing_length() {
        for length in 0..8_u8 {
            let data = (0..length)
                .map(|value| value.wrapping_mul(37).wrapping_add(250))
                .collect::<Vec<_>>();

            let encoded = encode_base64(&data);

            assert_eq!(decode_base64(&encoded).unwrap(), data);
        }
        assert!(decode_base64("1.0").is_none());
    }

    #[test]
    fn tree_binary_preserves_every_value_kind() {
        let tree = PluginStateTree {
            tree_type: "Root".into(),
            properties: [
                PluginStateValue::Bool(true),
                PluginStateValue::Integer(-7),
                PluginStateValue::Double(0.25),
                PluginStateValue::Text("日本語".into()),
                PluginStateValue::Encoded(PluginStateEncodedValue::Void),
                PluginStateValue::Encoded(PluginStateEncodedValue::Int64 { value: 1 << 40 }),
                PluginStateValue::Encoded(PluginStateEncodedValue::Binary {
                    base64: vec![0, 255],
                }),
                PluginStateValue::Encoded(PluginStateEncodedValue::Opaque {
                    marker: 7,
                    base64: vec![1, 2, 3],
                }),
            ]
            .into_iter()
            .enumerate()
            .map(|(index, value)| PluginStateProperty {
                name: format!("p{index}"),
                value,
            })
            .collect(),
            children: vec![PluginStateTree {
                tree_type: "Child".into(),
                properties: Vec::new(),
                children: Vec::new(),
            }],
        };
        let mut encoded = Vec::new();

        write_tree(&tree, &mut encoded);
        let (decoded, used) = read_tree(&encoded).unwrap();

        assert_eq!(decoded, tree);
        assert_eq!(used, encoded.len());
    }
}
