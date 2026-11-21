//! Renders a Score document as YAML.
//!
//! The document is a `serde_json::Value` built by the caller. Rendering is
//! deterministic: the same value always produces the same text. Maps and
//! sequences that contain only scalars and fit on one line are written in flow
//! form; everything else is written as blocks.

use serde_json::Value;

/// Longest flow-form collection, including its brackets.
const INLINE_LIMIT: usize = 96;

/// Renders one Score document as YAML text.
pub(crate) fn yaml(document: &Value) -> String {
    let Value::Object(fields) = document else {
        return format!("{}\n", scalar(document));
    };
    let mut output = String::new();
    map_block(fields, 0, &mut output);
    output
}

fn map_block(fields: &serde_json::Map<String, Value>, indent: usize, output: &mut String) {
    for (key, value) in fields {
        push_indent(indent, output);
        output.push_str(&key_text(key));
        if let Some(text) = inline(value) {
            output.push_str(": ");
            output.push_str(&text);
            output.push('\n');
            continue;
        }
        match value {
            Value::Object(fields) if fields.is_empty() => {
                output.push_str(": {}\n");
            }
            Value::Array(items) if items.is_empty() => {
                output.push_str(": []\n");
            }
            Value::Object(fields) => {
                output.push_str(":\n");
                map_block(fields, indent + 2, output);
            }
            Value::Array(items) => {
                output.push_str(":\n");
                seq_block(items, indent + 2, output);
            }
            _ => {
                output.push_str(": ");
                output.push_str(&scalar(value));
                output.push('\n');
            }
        }
    }
}

fn seq_block(items: &[Value], indent: usize, output: &mut String) {
    for item in items {
        push_indent(indent, output);
        if let Some(text) = inline(item) {
            output.push_str("- ");
            output.push_str(&text);
            output.push('\n');
            continue;
        }
        match item {
            Value::Object(fields) if fields.is_empty() => output.push_str("- {}\n"),
            Value::Array(items) if items.is_empty() => output.push_str("- []\n"),
            Value::Object(fields) => {
                // The first key shares the item line so that a list of maps
                // stays visually attached to its dashes.
                let mut nested = String::new();
                map_block(fields, indent + 2, &mut nested);
                if let Some(rest) = nested.strip_prefix(&" ".repeat(indent + 2)) {
                    output.push_str("- ");
                    output.push_str(rest);
                } else {
                    output.push_str("-\n");
                    output.push_str(&nested);
                }
            }
            Value::Array(items) => {
                output.push_str("-\n");
                seq_block(items, indent + 2, output);
            }
            _ => {
                output.push_str("- ");
                output.push_str(&scalar(item));
                output.push('\n');
            }
        }
    }
}

fn push_indent(indent: usize, output: &mut String) {
    output.extend(std::iter::repeat_n(' ', indent));
}

/// Renders a value in flow form when its whole subtree fits on one line.
fn inline(value: &Value) -> Option<String> {
    let text = inline_text(value)?;
    (text.len() <= INLINE_LIMIT).then_some(text)
}

fn inline_text(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) => {
            let mut text = String::from("[");
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    text.push_str(", ");
                }
                text.push_str(&inline_text(item)?);
            }
            text.push(']');
            Some(text)
        }
        Value::Object(fields) => {
            let mut text = String::from("{");
            for (index, (key, value)) in fields.iter().enumerate() {
                if index > 0 {
                    text.push_str(", ");
                }
                text.push_str(&key_text(key));
                text.push_str(": ");
                text.push_str(&inline_text(value)?);
            }
            text.push('}');
            Some(text)
        }
        other => Some(scalar(other)),
    }
}

fn key_text(key: &str) -> String {
    plain(key).map_or_else(|| quoted(key), str::to_owned)
}

fn scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => plain(value).map_or_else(|| quoted(value), str::to_owned),
        Value::Array(_) | Value::Object(_) => inline_text(value).unwrap_or_else(|| "{}".into()),
    }
}

fn quoted(value: &str) -> String {
    serde_json::to_string(value).expect("strings serialize")
}

/// Returns the text of a plain scalar when the parser reads it back verbatim.
fn plain(value: &str) -> Option<&str> {
    if value.is_empty()
        || !value
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '/'))
        || matches!(value, "true" | "false" | "null")
        || value.parse::<f64>().is_ok()
    {
        return None;
    }
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::score::Source;
    use serde_json::json;

    fn reparse(text: &str) -> Value {
        crate::score::parse::parse(text, &mut Source::default()).expect("emitted YAML parses")
    }

    #[test]
    fn scalar_only_collections_use_flow_form_and_the_rest_use_blocks() {
        let document = json!({
            "meta": {"title": "Song", "bpm": 120},
            "form": [
                {"section": "intro", "bars": 4, "tonic": "C", "scale": "ionian"},
                {"section": "verse", "bars": 8, "tonic": "A", "scale": "aeolian"}
            ],
            "sections": {
                "intro": {
                    "notes": "A long enough description that the map cannot share a line.",
                    "lines": {"main": {"instrument": "lead", "melody": "intro_line"}}
                }
            },
            "melodies": {"intro_line": ["1:16 3:16", "5:16 7:16"]}
        });

        let text = yaml(&document);

        assert!(
            text.contains("form:\n  - {section: intro, bars: 4, tonic: C, scale: ionian}\n"),
            "{text}"
        );
        assert!(
            text.contains("    lines: {main: {instrument: lead, melody: intro_line}}\n"),
            "{text}"
        );
        assert!(
            text.contains("melodies: {intro_line: [\"1:16 3:16\", \"5:16 7:16\"]}\n"),
            "{text}"
        );
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn values_that_read_differently_are_quoted() {
        let document = json!({
            "meta": {"title": "021", "bpm": 120.0},
            "values": [
                "1:4", "#4", "x... X...", "true", "", " score", "a: b",
                "3/4", "verse_2", "b2", "-1/8", "C"
            ]
        });

        let text = yaml(&document);

        assert!(text.contains("title: \"021\""), "{text}");
        assert!(text.contains("bpm: 120.0"), "{text}");
        for quoted in [
            "\"1:4\"",
            "\"#4\"",
            "\"x... X...\"",
            "\"true\"",
            "\"\"",
            "\" score\"",
            "\"a: b\"",
            "\"-1/8\"",
        ] {
            assert!(text.contains(quoted), "{quoted} in {text}");
        }
        for plain in ["3/4", "verse_2", "b2", "C"] {
            assert!(text.contains(plain), "{plain} in {text}");
        }
    }

    #[test]
    fn emitted_documents_read_back_as_the_same_value() {
        let document = json!({
            "meta": {"title": "Round trip", "bpm": 128.5, "meter": "6/8"},
            "form": [
                {"section": "a", "bars": 2, "tonic": "D", "scale": "dorian"},
                {"section": "b", "bars": 1, "tonic": "D"}
            ],
            "sections": {
                "a": {
                    "lines": {"main": {"instrument": "lead", "melody": "a_line"}},
                    "chords": ["1:m7", "<4:7"],
                    "events": [
                        {"kind": "cc", "instrument": "lead", "at": {"bar": 1, "step": 3}, "number": 11, "value": 96},
                        {"kind": "automation", "instrument": "lead", "at": {"bar": 2, "offset_quarters": "1/8"}, "duration_quarters": "1", "control": "pitch_bend", "from_value": -1.0, "to_value": 1.0}
                    ]
                },
                "b": {"notes": "ほかの調"}
            },
            "melodies": {"a_line": {"items": [".:4 1:12", "3:16"], "pickup_steps": 0}},
            "instruments": {
                "lead": {"source": {"vst3": {"plugin": "Synth", "state": "states/lead.json"}}, "gain_db": -3.5}
            },
            "mix": {"tracks": {"lead": {"output": "music", "sends": {"room": -9}}}}
        });

        let text = yaml(&document);

        assert_eq!(reparse(&text), document);
        assert_eq!(yaml(&reparse(&text)), text);
    }
}
