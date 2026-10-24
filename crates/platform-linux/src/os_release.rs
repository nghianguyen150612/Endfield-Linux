use std::collections::BTreeMap;

use crate::errors::OsReleaseParseError;

pub const MAX_OS_RELEASE_BYTES: usize = 64 * 1024;
const MAX_LINE_BYTES: usize = 4096;
const MAX_VALUE_BYTES: usize = 1024;
const MAX_ID_LIKE_VALUES: usize = 16;

/// The selected, bounded fields understood by P011. Unrelated keys are parsed
/// for syntax and then discarded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OsReleaseData {
    pub id: Option<String>,
    pub name: Option<String>,
    pub pretty_name: Option<String>,
    pub version_id: Option<String>,
    pub version: Option<String>,
    pub id_like: Vec<String>,
    pub variant_id: Option<String>,
}

/// Parses an os-release document without shell evaluation or interpolation.
/// For duplicate keys, the last syntactically valid assignment wins.
pub fn parse_os_release(bytes: &[u8]) -> Result<OsReleaseData, OsReleaseParseError> {
    if bytes.len() > MAX_OS_RELEASE_BYTES {
        return Err(OsReleaseParseError::TooLarge);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| OsReleaseParseError::InvalidUtf8)?;
    let mut fields = BTreeMap::<String, String>::new();

    for raw_line in text.split('\n') {
        if raw_line.len() > MAX_LINE_BYTES {
            return Err(OsReleaseParseError::LineTooLong);
        }
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, raw_value) = line
            .split_once('=')
            .ok_or(OsReleaseParseError::MalformedAssignment)?;
        if !valid_key(key) {
            return Err(OsReleaseParseError::InvalidKey);
        }
        let value = parse_value(raw_value.trim())?;
        if value.len() > MAX_VALUE_BYTES {
            return Err(OsReleaseParseError::ValueTooLong);
        }
        fields.insert(key.to_owned(), value);
    }

    let id = selected(&fields, "ID")?.map(|value| value.to_ascii_lowercase());
    let name = selected(&fields, "NAME")?;
    let pretty_name = selected(&fields, "PRETTY_NAME")?;
    let version_id = selected(&fields, "VERSION_ID")?;
    let version = selected(&fields, "VERSION")?;
    let variant_id = selected(&fields, "VARIANT_ID")?;
    let id_like = match selected(&fields, "ID_LIKE")? {
        None => Vec::new(),
        Some(value) => {
            let values = value
                .split_ascii_whitespace()
                .map(str::to_ascii_lowercase)
                .collect::<Vec<_>>();
            if values.len() > MAX_ID_LIKE_VALUES || values.iter().any(|value| !valid_id(value)) {
                return Err(OsReleaseParseError::TooManyIdLikeValues);
            }
            values
        }
    };

    if id.as_deref().is_some_and(|value| !valid_id(value))
        || [
            name.as_deref(),
            pretty_name.as_deref(),
            version_id.as_deref(),
            version.as_deref(),
            variant_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        .any(|value| !valid_text(value))
    {
        return Err(OsReleaseParseError::InvalidSelectedField);
    }

    Ok(OsReleaseData {
        id,
        name,
        pretty_name,
        version_id,
        version,
        id_like,
        variant_id,
    })
}

fn selected(
    fields: &BTreeMap<String, String>,
    key: &str,
) -> Result<Option<String>, OsReleaseParseError> {
    let Some(value) = fields.get(key) else {
        return Ok(None);
    };
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > 128 {
        return Err(OsReleaseParseError::InvalidSelectedField);
    }
    Ok(Some(value.clone()))
}

fn valid_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    matches!(bytes.next(), Some(b'A'..=b'Z'))
        && bytes.all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._-".contains(&byte)
        })
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().all(|character| !character.is_control())
}

fn parse_value(raw: &str) -> Result<String, OsReleaseParseError> {
    if raw.starts_with('"') {
        return parse_quoted(raw, '"');
    }
    if raw.starts_with('\'') {
        return parse_quoted(raw, '\'');
    }

    let mut value = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => {
                let escaped = characters
                    .next()
                    .ok_or(OsReleaseParseError::InvalidEscape)?;
                if !matches!(escaped, '\\' | '"' | '\'' | '$' | '`' | ' ') {
                    return Err(OsReleaseParseError::InvalidEscape);
                }
                value.push(escaped);
            }
            character if character.is_whitespace() => {
                return Err(OsReleaseParseError::InvalidValue);
            }
            '"' | '\'' | '$' | '`' => return Err(OsReleaseParseError::InvalidValue),
            character if character.is_control() => return Err(OsReleaseParseError::InvalidValue),
            character => value.push(character),
        }
        if value.len() > MAX_VALUE_BYTES {
            return Err(OsReleaseParseError::ValueTooLong);
        }
    }
    Ok(value)
}

fn parse_quoted(raw: &str, quote: char) -> Result<String, OsReleaseParseError> {
    let mut value = String::with_capacity(raw.len());
    let mut characters = raw.chars();
    let _ = characters.next();
    let mut closed = false;
    while let Some(character) = characters.next() {
        if character == quote {
            closed = true;
            break;
        }
        if quote == '"' && character == '\\' {
            let escaped = characters
                .next()
                .ok_or(OsReleaseParseError::InvalidEscape)?;
            if !matches!(escaped, '\\' | '"' | '$' | '`') {
                return Err(OsReleaseParseError::InvalidEscape);
            }
            value.push(escaped);
        } else if character.is_control() {
            return Err(OsReleaseParseError::InvalidValue);
        } else {
            value.push(character);
        }
        if value.len() > MAX_VALUE_BYTES {
            return Err(OsReleaseParseError::ValueTooLong);
        }
    }
    if !closed {
        return Err(OsReleaseParseError::UnterminatedQuote);
    }
    if characters.any(|character| !character.is_whitespace()) {
        return Err(OsReleaseParseError::TrailingCharacters);
    }
    Ok(value)
}
