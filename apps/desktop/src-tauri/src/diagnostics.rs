use serde::Serialize;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_LOG_BYTES: u64 = 1_048_576;
const MAX_MESSAGE_CHARS: usize = 512;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DiagnosticEvent<'a> {
    timestamp_ms: u128,
    scope: &'a str,
    message: String,
}

pub fn record(root: &Path, scope: &str, message: &str) -> io::Result<()> {
    let directory = root.join("logs");
    fs::create_dir_all(&directory)?;
    let path = directory.join("riffra.log.jsonl");
    if path
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or_default()
        >= MAX_LOG_BYTES
    {
        let rotated = directory.join("riffra.log.jsonl.1");
        let _ = fs::remove_file(&rotated);
        fs::rename(&path, rotated)?;
    }
    let event = DiagnosticEvent {
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        scope: sanitize_scope(scope),
        message: sanitize_message(message),
    };
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    serde_json::to_writer(&mut file, &event)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    file.write_all(b"\n")
}

fn sanitize_scope(scope: &str) -> &str {
    if scope.len() <= 64
        && scope
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        scope
    } else {
        "unknown"
    }
}

pub fn sanitize_message(message: &str) -> String {
    let normalized = message
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let mut output = String::with_capacity(normalized.len().min(MAX_MESSAGE_CHARS));
    let mut redact_next = false;
    for token in normalized.split_whitespace() {
        let lower = token.to_ascii_lowercase();
        let sensitive_key = lower.contains("apikey")
            || lower.contains("api-key")
            || lower.contains("token")
            || lower.contains("secret")
            || lower.contains("password");
        let is_path = token.contains('\\') || token.contains('/') || token.contains(":\\");
        let safe = if redact_next {
            redact_next = false;
            "[redacted]"
        } else if sensitive_key {
            let has_inline_value = token
                .split_once(['=', ':'])
                .is_some_and(|(_, value)| !value.is_empty());
            if !has_inline_value {
                redact_next = true;
            }
            "[redacted]"
        } else if is_path {
            "<path>"
        } else {
            token
        };
        if !output.is_empty() {
            output.push(' ');
        }
        output.push_str(safe);
        if output.chars().count() >= MAX_MESSAGE_CHARS {
            break;
        }
    }
    output.chars().take(MAX_MESSAGE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn redacts_secrets_paths_and_control_characters() {
        let message =
            sanitize_message("apiKey=secret-value C:\\Users\\artist\\take.wav\nsecond line");
        assert!(!message.contains("secret-value"));
        assert!(!message.contains("C:\\Users"));
        assert!(message.contains("[redacted]"));
        assert!(message.contains("<path>"));
        assert!(!message.contains('\n'));
    }

    #[test]
    fn redacts_secrets_when_key_and_value_are_separate_tokens() {
        let message = sanitize_message("apiKey: secret-value token another-secret");
        assert!(!message.contains("secret-value"));
        assert!(!message.contains("another-secret"));
        assert_eq!(message.matches("[redacted]").count(), 4);
    }

    #[test]
    fn rotates_large_logs_without_unbounded_growth() {
        let root = std::env::temp_dir().join(format!("riffra-diagnostics-{}", std::process::id()));
        fs::create_dir_all(root.join("logs")).unwrap();
        let path = root.join("logs/riffra.log.jsonl");
        let file = File::create(&path).unwrap();
        file.set_len(MAX_LOG_BYTES).unwrap();
        record(&root, "job", "failure").unwrap();
        assert!(root.join("logs/riffra.log.jsonl.1").is_file());
        assert!(path.metadata().unwrap().len() < MAX_LOG_BYTES);
        let _ = fs::remove_dir_all(root);
    }
}
