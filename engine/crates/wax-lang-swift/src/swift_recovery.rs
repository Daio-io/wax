//! Byte-preserving recovery for Swift syntax unsupported by the bundled grammar.

/// A Swift source buffer with recoverable syntax masked for tree-sitter.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NormalizedSwiftSource {
    /// Source bytes passed to tree-sitter.
    pub(crate) bytes: Vec<u8>,
    /// Number of `@available(...)` attributes masked before `#Preview`.
    pub(crate) recovered_available_preview_count: u32,
}

/// Masks `@available(...)` attributes immediately followed by `#Preview`.
pub(crate) fn normalize_swift_source(source: &[u8]) -> NormalizedSwiftSource {
    let mut bytes = source.to_vec();
    let mut recovered_available_preview_count = 0_u32;
    let mut index = 0;

    while index < source.len() {
        if let Some(end) = skip_comment_or_string(source, index) {
            index = end;
            continue;
        }

        if starts_token(source, index, b"@available")
            && let Some(attribute_end) = available_attribute_end(source, index)
        {
            let preview_start = skip_trivia(source, attribute_end);
            if starts_token(source, preview_start, b"#Preview") {
                mask_non_newline_bytes(&mut bytes, index, attribute_end);
                recovered_available_preview_count =
                    recovered_available_preview_count.saturating_add(1);
                index = attribute_end;
                continue;
            }
        }

        index += 1;
    }

    NormalizedSwiftSource {
        bytes,
        recovered_available_preview_count,
    }
}

fn available_attribute_end(source: &[u8], start: usize) -> Option<usize> {
    let mut index = skip_trivia(source, start + b"@available".len());
    if source.get(index) != Some(&b'(') {
        return None;
    }

    let mut depth = 0_u32;
    while index < source.len() {
        if let Some(end) = skip_comment_or_string(source, index) {
            index = end;
            continue;
        }

        match source[index] {
            b'(' => depth = depth.saturating_add(1),
            b')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index + 1);
                }
            }
            _ => {}
        }
        index += 1;
    }

    None
}

fn skip_trivia(source: &[u8], mut index: usize) -> usize {
    while index < source.len() {
        if source[index].is_ascii_whitespace() {
            index += 1;
        } else if let Some(end) = skip_comment(source, index) {
            index = end;
        } else {
            break;
        }
    }
    index
}

fn skip_comment_or_string(source: &[u8], index: usize) -> Option<usize> {
    skip_comment(source, index).or_else(|| skip_string(source, index))
}

fn skip_comment(source: &[u8], index: usize) -> Option<usize> {
    if source.get(index..index + 2) == Some(b"//") {
        let mut end = index + 2;
        while end < source.len() && !matches!(source[end], b'\r' | b'\n') {
            end += 1;
        }
        return Some(end);
    }

    if source.get(index..index + 2) != Some(b"/*") {
        return None;
    }

    let mut depth = 1_u32;
    let mut cursor = index + 2;
    while cursor < source.len() {
        if source.get(cursor..cursor + 2) == Some(b"/*") {
            depth = depth.saturating_add(1);
            cursor += 2;
        } else if source.get(cursor..cursor + 2) == Some(b"*/") {
            depth = depth.checked_sub(1)?;
            cursor += 2;
            if depth == 0 {
                return Some(cursor);
            }
        } else {
            cursor += 1;
        }
    }

    Some(source.len())
}

fn skip_string(source: &[u8], index: usize) -> Option<usize> {
    let (hash_count, quote_length) = string_start(source, index)?;
    let mut cursor = index + hash_count + quote_length;
    while cursor < source.len() {
        if hash_count == 0 && source[cursor] == b'\\' {
            cursor = (cursor + 2).min(source.len());
            continue;
        }

        if matches_repeated(source, cursor, b'"', quote_length)
            && matches_repeated(source, cursor + quote_length, b'#', hash_count)
        {
            return Some(cursor + quote_length + hash_count);
        }
        cursor += 1;
    }

    Some(source.len())
}

fn string_start(source: &[u8], index: usize) -> Option<(usize, usize)> {
    if source.get(index) == Some(&b'"') {
        let quote_length = if source.get(index..index + 3) == Some(b"\"\"\"") {
            3
        } else {
            1
        };
        return Some((0, quote_length));
    }

    if source.get(index) != Some(&b'#') {
        return None;
    }

    let mut hash_count = 0;
    while source.get(index + hash_count) == Some(&b'#') {
        hash_count += 1;
    }
    let quote_index = index + hash_count;
    if source.get(quote_index) != Some(&b'"') {
        return None;
    }
    let quote_length = if source.get(quote_index..quote_index + 3) == Some(b"\"\"\"") {
        3
    } else {
        1
    };
    Some((hash_count, quote_length))
}

fn starts_token(source: &[u8], index: usize, token: &[u8]) -> bool {
    source.get(index..index + token.len()) == Some(token)
        && source
            .get(index + token.len())
            .is_none_or(|byte| !is_identifier_byte(*byte))
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn matches_repeated(source: &[u8], start: usize, byte: u8, count: usize) -> bool {
    source
        .get(start..start.saturating_add(count))
        .is_some_and(|slice| slice.iter().all(|value| *value == byte))
}

fn mask_non_newline_bytes(bytes: &mut [u8], start: usize, end: usize) {
    for byte in &mut bytes[start..end] {
        if !matches!(*byte, b'\r' | b'\n') {
            *byte = b' ';
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{NormalizedSwiftSource, normalize_swift_source};

    #[test]
    fn masks_available_attribute_before_preview_without_changing_offsets() {
        let source = b"@available(iOS 18.0, *)\r\n#Preview { }\r\n";

        let normalized = normalize_swift_source(source);

        assert_eq!(normalized.recovered_available_preview_count, 1);
        assert_eq!(normalized.bytes.len(), source.len());
        assert_eq!(normalized.bytes[23], b'\r');
        assert_eq!(normalized.bytes[24], b'\n');
        assert_eq!(&normalized.bytes[25..33], b"#Preview");
    }

    #[test]
    fn leaves_available_attribute_on_declaration_unchanged() {
        let source = b"@available(iOS 18.0, *)\nstruct Card {}\n";

        assert_eq!(
            normalize_swift_source(source),
            NormalizedSwiftSource {
                bytes: source.to_vec(),
                recovered_available_preview_count: 0,
            }
        );
    }

    #[test]
    fn ignores_attribute_like_text_in_comments_and_strings() {
        let source = br##"// @available(iOS 18.0, *)
let text = "@available(iOS 18.0, *) #Preview"
let escaped = "escaped \" @available(iOS 18.0, *) #Preview"
/* outer /* @available(iOS 18.0, *) #Preview */ comment */
"#available(iOS 18.0, *) #Preview"
"##;

        assert_eq!(
            normalize_swift_source(source).recovered_available_preview_count,
            0
        );
    }

    #[test]
    fn ignores_unbalanced_available_attribute() {
        let source = b"@available(iOS 18.0, *\n#Preview { }\n";

        assert_eq!(
            normalize_swift_source(source).recovered_available_preview_count,
            0
        );
    }

    #[test]
    fn ignores_unrelated_freestanding_macros() {
        let source = b"@available(iOS 18.0, *)\n#OtherMacro { }\n";

        assert_eq!(
            normalize_swift_source(source).recovered_available_preview_count,
            0
        );
    }

    #[test]
    fn skips_raw_and_multiline_strings_while_searching() {
        let source = br###"let raw = #"@available(iOS 18.0, *) #Preview"#
let multiline = """
@available(iOS 18.0, *)
#Preview { }
"""
"###;

        assert_eq!(
            normalize_swift_source(source).recovered_available_preview_count,
            0
        );
    }
}
