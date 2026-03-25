/// Check if `text` contains `word` at a word boundary (not part of a larger identifier).
/// Word boundaries are non-alphanumeric, non-underscore characters or string edges.
pub fn contains_word_boundary(text: &str, word: &str) -> bool {
    if word.is_empty() {
        return false;
    }
    let text_bytes = text.as_bytes();
    let word_len = word.len();
    let mut start = 0;
    while start + word_len <= text.len() {
        match text[start..].find(word) {
            Some(pos) => {
                let abs_pos = start + pos;
                let before_ok = abs_pos == 0 || {
                    let b = text_bytes[abs_pos - 1];
                    !b.is_ascii_alphanumeric() && b != b'_'
                };
                let after_pos = abs_pos + word_len;
                let after_ok = after_pos >= text.len() || {
                    let b = text_bytes[after_pos];
                    !b.is_ascii_alphanumeric() && b != b'_'
                };
                if before_ok && after_ok {
                    return true;
                }
                start = abs_pos + 1;
            }
            None => break,
        }
    }
    false
}
