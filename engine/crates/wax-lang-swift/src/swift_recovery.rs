//! Byte-preserving recovery for Swift syntax unsupported by the bundled grammar.

/// A Swift source buffer with recoverable syntax masked for tree-sitter.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NormalizedSwiftSource {
    /// Source bytes passed to tree-sitter.
    pub(crate) bytes: Vec<u8>,
}

/// Recovers unsupported Swift syntax without changing source byte offsets.
pub(crate) fn normalize_swift_source(source: &[u8]) -> NormalizedSwiftSource {
    let mut bytes = source.to_vec();
    let mut index = 0;
    let mut previous_significant = None;

    while index < source.len() {
        if let Some(end) = skip_comment(source, index) {
            index = end;
            continue;
        }
        if let Some(end) = skip_string(source, index) {
            previous_significant = Some((index, end));
            index = end;
            continue;
        }

        if source.get(index) == Some(&b'@') {
            let attribute_end = preview_attribute_prefix(source, index);
            let preview_start = skip_trivia(source, attribute_end);
            if attribute_end != index && starts_token(source, preview_start, b"#Preview") {
                mask_non_newline_bytes(&mut bytes, index, attribute_end);
                index = attribute_end;
                continue;
            }
        }

        if source.get(index) == Some(&b'(')
            && let Some(end) = unit_expression_end(source, index)
            && can_recover_unit_expression(source, index, end, previous_significant)
        {
            bytes[index] = b'{';
            bytes[end - 1] = b'}';
            index = end;
            continue;
        }

        if source[index].is_ascii_whitespace() {
            index += 1;
        } else if let Some((start, end)) = next_word(source, index) {
            previous_significant = Some((start, end));
            index = end;
        } else {
            previous_significant = Some((index, index + 1));
            index += 1;
        }
    }

    NormalizedSwiftSource { bytes }
}

fn unit_expression_end(source: &[u8], opening_index: usize) -> Option<usize> {
    let mut index = opening_index + 1;
    while index < source.len() {
        if let Some(end) = skip_comment(source, index) {
            index = end;
        } else if skip_string(source, index).is_some() {
            return None;
        } else if source[index].is_ascii_whitespace() {
            index += 1;
        } else if source[index] == b')' {
            return Some(index + 1);
        } else {
            return None;
        }
    }
    None
}

fn can_recover_unit_expression(
    source: &[u8],
    opening_index: usize,
    end: usize,
    previous_significant: Option<(usize, usize)>,
) -> bool {
    if follows_function_type(source, end) || precedes_type_annotation(source, opening_index) {
        return false;
    }

    let Some((start, token_end)) = previous_significant else {
        return false;
    };

    !is_pattern_context(source, opening_index)
        && !can_end_callable_expression(&source[start..token_end])
}

fn is_pattern_context(source: &[u8], opening_index: usize) -> bool {
    let previous = previous_token(source, opening_index);
    if matches!(previous, Some(b"case" | b"let" | b"var")) {
        return true;
    }

    if source[..opening_index]
        .iter()
        .rev()
        .find(|byte| !byte.is_ascii_whitespace())
        .is_some_and(|byte| *byte == b',')
    {
        let line_start = source[..opening_index]
            .iter()
            .rposition(|byte| *byte == b'\n')
            .map_or(0, |index| index + 1);
        return source[line_start..opening_index]
            .windows(4)
            .any(|window| window == b"case");
    }

    false
}

fn previous_token(source: &[u8], index: usize) -> Option<&[u8]> {
    let mut end = index;
    while end > 0 && source[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let start = source[..end]
        .iter()
        .rposition(|byte| !is_word_byte(*byte))
        .map_or(0, |position| position + 1);
    (start < end).then_some(&source[start..end])
}

fn precedes_type_annotation(source: &[u8], opening_index: usize) -> bool {
    let mut index = opening_index;
    while index > 0 && source[index - 1].is_ascii_whitespace() {
        index -= 1;
    }
    if index >= 1 && source[index - 1] == b':' {
        return !is_call_argument_label(source, index - 1);
    }
    if index >= 2 && source[index - 2..index] == *b"->" {
        return true;
    }

    if index >= 2 && source[index - 2..index] == *b"?." {
        return true;
    }

    if index >= 1 && source[index - 1] == b'(' {
        return true;
    }

    matches!(source.get(index.wrapping_sub(1)), Some(b'<') | Some(b'['))
        || is_nested_type_tuple(source, opening_index)
        || is_typealias_value(source, opening_index)
        || is_where_or_associated_type(source, opening_index)
}

fn is_where_or_associated_type(source: &[u8], opening_index: usize) -> bool {
    let line_start = source[..opening_index]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    let line = &source[line_start..opening_index];
    line.windows(5).any(|window| window == b"where")
        || line.windows(14).any(|window| window == b"associatedtype")
}

fn is_call_argument_label(source: &[u8], colon: usize) -> bool {
    let mut depth = 0;
    let mut index = colon;
    while index > 0 {
        index -= 1;
        match source[index] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
            b'(' if depth == 0 => {
                let prefix = &source[..index];
                let mut words = prefix
                    .split(|byte| !is_identifier_byte(*byte))
                    .filter(|word| !word.is_empty());
                let _function_name = words.next_back();
                return words.next_back() != Some(b"func");
            }
            _ => {}
        }
    }
    false
}

fn is_nested_type_tuple(source: &[u8], opening_index: usize) -> bool {
    let mut depth = 0;
    let mut index = opening_index;
    while index > 0 {
        index -= 1;
        match source[index] {
            b')' => depth += 1,
            b'(' if depth > 0 => depth -= 1,
            b'(' if depth == 0 => {
                let mut before = index;
                while before > 0 && source[before - 1].is_ascii_whitespace() {
                    before -= 1;
                }
                return before > 0
                    && (matches!(source[before - 1], b':' | b'<' | b'[')
                        && (source[before - 1] != b':'
                            || !is_call_argument_label(source, before - 1)));
            }
            _ => {}
        }
    }
    false
}

fn is_typealias_value(source: &[u8], opening_index: usize) -> bool {
    let mut before = opening_index;
    while before > 0 && source[before - 1].is_ascii_whitespace() {
        before -= 1;
    }
    if source.get(before.wrapping_sub(1)) != Some(&b'=') {
        return false;
    }
    let line_start = source[..before]
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    source[line_start..before]
        .windows(9)
        .any(|window| window == b"typealias")
}

fn follows_function_type(source: &[u8], end: usize) -> bool {
    let mut index = skip_trivia(source, end);
    loop {
        if source.get(index..index + 2) == Some(b"->") {
            return true;
        }

        let Some((token_start, token_end)) = next_word(source, index) else {
            return false;
        };
        if !matches!(
            &source[token_start..token_end],
            b"async" | b"throws" | b"rethrows"
        ) {
            return false;
        }
        index = skip_trivia(source, token_end);
    }
}

fn next_word(source: &[u8], start: usize) -> Option<(usize, usize)> {
    if !is_word_byte(*source.get(start)?) {
        return None;
    }
    let mut end = start + 1;
    while source.get(end).is_some_and(|byte| is_word_byte(*byte)) {
        end += 1;
    }
    Some((start, end))
}

fn can_end_callable_expression(token: &[u8]) -> bool {
    if token.len() == 1 {
        return matches!(token[0], b')' | b']' | b'}' | b'?' | b'!') || is_word_byte(token[0]);
    }
    if token
        .first()
        .is_some_and(|byte| *byte == b'"' || *byte == b'#')
    {
        return true;
    }
    if token.iter().all(|byte| is_word_byte(*byte)) {
        return !matches!(
            token,
            b"return"
                | b"throw"
                | b"yield"
                | b"case"
                | b"else"
                | b"do"
                | b"defer"
                | b"guard"
                | b"if"
                | b"while"
                | b"for"
                | b"switch"
                | b"catch"
                | b"repeat"
                | b"in"
                | b"is"
                | b"as"
                | b"let"
                | b"var"
                | b"await"
                | b"try"
                | b"some"
                | b"any"
                | b"async"
                | b"throws"
                | b"rethrows"
        );
    }
    false
}

fn is_word_byte(byte: u8) -> bool {
    is_identifier_byte(byte) || byte >= 0x80
}

fn preview_attribute_prefix(source: &[u8], start: usize) -> usize {
    let mut end = start;
    while let Some(attribute_end) = attribute_end(source, end) {
        end = skip_trivia(source, attribute_end);
    }
    end
}

fn attribute_end(source: &[u8], start: usize) -> Option<usize> {
    if source.get(start) != Some(&b'@') {
        return None;
    }
    let name_end = source[start + 1..]
        .iter()
        .position(|byte| !is_identifier_byte(*byte))
        .map_or(source.len(), |offset| start + 1 + offset);
    if name_end == start + 1 {
        return None;
    }
    let index = skip_trivia(source, name_end);
    if source.get(index) != Some(&b'(') {
        return Some(name_end);
    }
    let argument_start = index + 1;
    balanced_delimited_end(source, index, argument_start)
}

fn balanced_delimited_end(
    source: &[u8],
    opening_index: usize,
    argument_start: usize,
) -> Option<usize> {
    let mut index = opening_index;

    let mut delimiters = Vec::new();
    while index < source.len() {
        if let Some(end) = skip_comment_or_string(source, index) {
            index = end;
            continue;
        }

        match source[index] {
            b'(' | b'[' | b'{' => delimiters.push(source[index]),
            b')' | b']' | b'}' => {
                let opening = delimiters.pop()?;
                if !matches!(
                    (opening, source[index]),
                    (b'(', b')') | (b'[', b']') | (b'{', b'}')
                ) {
                    return None;
                }
                if delimiters.is_empty() {
                    if source.get(opening_index) == Some(&b'(')
                        && source
                            .get(argument_start..index)
                            .is_some_and(|arguments| !arguments.iter().all(u8::is_ascii_whitespace))
                    {
                        return Some(index + 1);
                    }
                    return None;
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

        assert_eq!(normalize_swift_source(source).bytes, source.to_vec());
    }

    #[test]
    fn ignores_unbalanced_available_attribute() {
        let source = b"@available(iOS 18.0, *\n#Preview { }\n";

        assert_eq!(normalize_swift_source(source).bytes, source.to_vec());
    }

    #[test]
    fn masks_contiguous_available_attributes_before_preview() {
        let source = b"@available(iOS 18, *)\n@available(macOS 15, *)\n#Preview { }\n";

        let normalized = normalize_swift_source(source);

        assert_eq!(&normalized.bytes[source.len() - 13..], b"#Preview { }\n");
    }

    #[test]
    fn masks_generic_attributes_between_available_and_preview() {
        let source = b"@available(iOS 18.0, *)\n@MainActor\n#Preview { }\n";

        let normalized = normalize_swift_source(source);

        assert_eq!(&normalized.bytes[source.len() - 13..], b"#Preview { }\n");
    }

    #[test]
    fn preserves_invalid_balanced_available_attribute() {
        let source = b"@available(])\n#Preview { }\n";

        let normalized = normalize_swift_source(source);

        assert_eq!(normalized.bytes, source);
    }

    #[test]
    fn preserves_empty_available_attribute() {
        let source = b"@available()\n#Preview { }\n";

        assert_eq!(normalize_swift_source(source).bytes, source);
    }

    #[test]
    fn masks_available_attribute_with_comment_trivia_before_preview() {
        let source = b"@available(iOS 18.0, *)\n// comment\n#Preview { }\n";

        let normalized = normalize_swift_source(source);

        assert_eq!(normalized.bytes.len(), source.len());
        assert_eq!(&normalized.bytes[source.len() - 13..], b"#Preview { }\n");
    }

    #[test]
    fn ignores_unrelated_freestanding_macros() {
        let source = b"@available(iOS 18.0, *)\n#OtherMacro { }\n";

        assert_eq!(normalize_swift_source(source).bytes, source.to_vec());
    }

    #[test]
    fn skips_raw_and_multiline_strings_while_searching() {
        let source = br###"let raw = #"@available(iOS 18.0, *) #Preview"#
let multiline = """
@available(iOS 18.0, *)
#Preview { }
"""
"###;

        assert_eq!(normalize_swift_source(source).bytes, source.to_vec());
    }

    #[test]
    fn normalizes_unit_expression_to_empty_closure_token() {
        let source = b"let value: Void = ();";

        assert_eq!(
            normalize_swift_source(source).bytes,
            b"let value: Void = {};"
        );
    }

    #[test]
    fn normalizes_unit_expression_after_return() {
        let source = b"func finish() { return () }";

        assert_eq!(
            normalize_swift_source(source).bytes,
            b"func finish() { return {} }"
        );
    }

    #[test]
    fn normalizes_labeled_and_nested_unit_expressions() {
        let source = b"resume(returning: (before, ()));";

        assert_eq!(
            normalize_swift_source(source).bytes,
            b"resume(returning: (before, {}));"
        );
    }

    #[test]
    fn preserves_newlines_inside_multiline_unit_expression() {
        let source = b"let value = (\n);";

        assert_eq!(normalize_swift_source(source).bytes, b"let value = {\n};");
    }

    #[test]
    fn leaves_callable_parentheses_and_function_types_unchanged() {
        let source = b"foo(); func f(); #macro(); let closure: () -> Void = {};";

        assert_eq!(normalize_swift_source(source).bytes, source.to_vec());
    }

    #[test]
    fn leaves_empty_tuple_types_unchanged() {
        let source = b"func returnsUnit() -> () {}\nfunc takesUnit(_ value: ()) {}";

        assert_eq!(normalize_swift_source(source).bytes, source.to_vec());
    }

    #[test]
    fn preserves_empty_tuple_types_in_aliases_generics_collections_and_nested_tuples() {
        let source = b"typealias Unit = ()\nlet a: [()] = []\nlet b: Result<(), Error> = fatalError()\nlet c: ((), Int) = fatalError()";

        assert_eq!(normalize_swift_source(source).bytes, source.to_vec());
    }

    #[test]
    fn preserves_unit_patterns_and_other_ambiguous_parentheses() {
        let source = b"foo?.()\nwhere T == ()\nassociatedtype T = ()\ncase (_, ())\nlet () = value\nprint(())";

        assert_eq!(normalize_swift_source(source).bytes, source.to_vec());
    }

    #[test]
    fn recovers_empty_tuple_labeled_call_arguments() {
        let source = b"continuation.resume(returning: ())";

        assert_eq!(
            normalize_swift_source(source).bytes,
            b"continuation.resume(returning: {})"
        );
    }

    #[test]
    fn ignores_unit_like_parentheses_in_comments_and_strings() {
        let source = br###"// ()
let text = "()"
/* () */
let value: Void = ();"###;

        assert_eq!(
            normalize_swift_source(source).bytes,
            br###"// ()
let text = "()"
/* () */
let value: Void = {};"###
        );
    }

    #[test]
    fn leaves_unbalanced_parentheses_unchanged() {
        let source = b"let value: Void = (";

        assert_eq!(normalize_swift_source(source).bytes, source.to_vec());
    }
}
