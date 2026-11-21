use serde_json::{Map, Value};
use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::{Marker, TScalarStyle};

use super::{Error, Result, Source};

pub(super) fn parse(text: &str, source: &mut Source) -> Result<Value> {
    text.clone_into(&mut source.text);
    let mut line_offsets = Vec::new();
    let mut columns = Vec::new();
    for (index, character) in text.char_indices() {
        columns.push(index);
        if character == '\n'
            || (character == '\r' && text.as_bytes().get(index + 1) != Some(&b'\n'))
        {
            line_offsets.push(std::mem::take(&mut columns));
        }
    }
    columns.push(text.len());
    line_offsets.push(columns);
    let mut parser = Parser::new_from_str(text);
    next(&mut parser)?;
    let (start, _) = next(&mut parser)?;
    if start != Event::DocumentStart {
        return Err(Error::new("", "expected one YAML document"));
    }
    let value = node(&mut parser, source, &line_offsets, "", 0)?;
    if next(&mut parser)?.0 != Event::DocumentEnd || next(&mut parser)?.0 != Event::StreamEnd {
        return Err(Error::new("", "expected exactly one YAML document"));
    }
    Ok(value)
}

fn next(parser: &mut Parser<std::str::Chars<'_>>) -> Result<(Event, Marker)> {
    parser.next_token().map_err(|error| {
        let mut result = Error::new("", error.to_string());
        result.line = error.marker().line();
        result.column = error.marker().col() + 1;
        result
    })
}

#[allow(clippy::too_many_lines)]
fn node(
    parser: &mut Parser<std::str::Chars<'_>>,
    source: &mut Source,
    line_offsets: &[Vec<usize>],
    path: &str,
    depth: usize,
) -> Result<Value> {
    if depth > 64 {
        return Err(Error::new(path, "YAML nesting exceeds 64 levels"));
    }
    let (event, mark) = next(parser)?;
    source
        .positions
        .insert(path.into(), (mark.line(), mark.col() + 1));
    let fail = |message: &str| {
        let mut error = Error::new(path, message);
        error.line = mark.line();
        error.column = mark.col() + 1;
        error
    };
    let byte_index = mark
        .line()
        .checked_sub(1)
        .and_then(|line| line_offsets.get(line))
        .and_then(|columns| columns.get(mark.col()))
        .copied()
        .ok_or_else(|| fail("YAML parser returned an invalid source position"))?;
    source.indices.insert(path.into(), byte_index);
    let value = match event {
        Event::Scalar(text, style, anchor, tag) => {
            let rational_field = matches!(
                path.rsplit('.').next(),
                Some(
                    "gate"
                        | "duration_steps"
                        | "duration_quarters"
                        | "offset_quarters"
                        | "at_quarters"
                        | "last_bar_quarters"
                )
            );
            if anchor != 0 || tag.is_some() {
                return Err(fail("YAML anchors and tags are not supported"));
            }
            if style == TScalarStyle::Plain && !rational_field {
                match text.as_str() {
                    "true" => Value::Bool(true),
                    "false" => Value::Bool(false),
                    "null" | "~" | "" => Value::Null,
                    _ => serde_json::from_str::<Value>(&text)
                        .ok()
                        .filter(Value::is_number)
                        .unwrap_or(Value::String(text)),
                }
            } else {
                Value::String(text)
            }
        }
        Event::SequenceStart(anchor, tag) => {
            if anchor != 0 || tag.is_some() {
                return Err(fail("YAML anchors and tags are not supported"));
            }
            let mut values = Vec::new();
            while parser.peek().map_err(|e| fail(&e.to_string()))?.0 != Event::SequenceEnd {
                values.push(node(
                    parser,
                    source,
                    line_offsets,
                    &format!("{path}[{}]", values.len()),
                    depth + 1,
                )?);
                if values.len() > 1_000_000 {
                    return Err(fail("sequence exceeds one million entries"));
                }
            }
            next(parser)?;
            Value::Array(values)
        }
        Event::MappingStart(anchor, tag) => {
            if anchor != 0 || tag.is_some() {
                return Err(fail("YAML anchors and tags are not supported"));
            }
            let mut map = Map::new();
            while parser.peek().map_err(|e| fail(&e.to_string()))?.0 != Event::MappingEnd {
                let (key, key_mark) = next(parser)?;
                let Event::Scalar(key, style, 0, None) = key else {
                    return Err(fail("mapping keys must be strings"));
                };
                let numeric_replace = path.ends_with(".replace") && key.parse::<usize>().is_ok();
                if key == "<<"
                    || (!numeric_replace
                        && style == TScalarStyle::Plain
                        && matches!(key.as_str(), "true" | "false" | "null" | "~"))
                    || (!numeric_replace
                        && style == TScalarStyle::Plain
                        && key.parse::<f64>().is_ok())
                {
                    return Err(fail(
                        "mapping keys must be strings; merge keys are not supported",
                    ));
                }
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                if map.contains_key(&key) {
                    let mut e = Error::new(child, "duplicate mapping key");
                    e.line = key_mark.line();
                    e.column = key_mark.col() + 1;
                    return Err(e);
                }
                let value = node(parser, source, line_offsets, &child, depth + 1)?;
                map.insert(key, value);
            }
            next(parser)?;
            Value::Object(map)
        }
        Event::Alias(_) => return Err(fail("YAML aliases are not supported")),
        _ => return Err(fail("expected a scalar, sequence, or mapping")),
    };
    if !value.is_object() && !value.is_array() {
        source.values.insert(path.into(), value.clone());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_scalars_preserve_unicode_and_following_source_positions() {
        for style in [">", ">-", "|", "|-"] {
            for newline in ["\n", "\r\n", "\r"] {
                let content = "日本語と音楽🎵".repeat(32);
                let text = format!(
                    "notes: {style}{newline}  {content}{newline}  続き{newline}next: invalid"
                );
                let mut source = Source::default();

                let value = parse(&text, &mut source).expect("Unicode block scalar");

                let separator = if style.starts_with('>') { " " } else { "\n" };
                let ending = if style.ends_with('-') { "" } else { "\n" };
                assert_eq!(value["notes"], format!("{content}{separator}続き{ending}"));
                assert_eq!(source.positions["notes"], (2, 3));
                assert_eq!(source.indices["notes"], text.find(&content).unwrap());
                assert_eq!(source.positions["next"], (4, 7));
                assert_eq!(source.indices["next"], text.find("invalid").unwrap());

                let malformed = text.replace("next: invalid", "next: \"unfinished");
                let error = parse(&malformed, &mut Source::default())
                    .expect_err("unterminated quote after a Unicode block scalar");
                assert_eq!((error.line, error.column), (4, 7));
            }
        }
    }
}
