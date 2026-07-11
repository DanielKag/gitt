//! Pure text helpers shared by the reducer (layout math) and the UI (rendering). No I/O.

/// Greedy word-wrap `text` to `width` columns (hard-breaking words longer than `width`), preserving
/// explicit newlines. Deterministic, so panel height can be measured the same way it is rendered.
pub fn wrap_words(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut line = String::new();
        for raw in para.split_whitespace() {
            let mut word = raw.to_string();
            // Hard-break a word that can't fit on any line.
            while word.chars().count() > width {
                if !line.is_empty() {
                    out.push(std::mem::take(&mut line));
                }
                out.push(word.chars().take(width).collect());
                word = word.chars().skip(width).collect();
            }
            let wlen = word.chars().count();
            if line.is_empty() {
                line = word;
            } else if line.chars().count() + 1 + wlen <= width {
                line.push(' ');
                line.push_str(&word);
            } else {
                out.push(std::mem::take(&mut line));
                line = word;
            }
        }
        out.push(line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::wrap_words;

    #[test]
    fn greedy_and_hard_break() {
        assert_eq!(wrap_words("a bb ccc", 5), vec!["a bb", "ccc"]);
        assert_eq!(wrap_words("abcdefg hi", 4), vec!["abcd", "efg", "hi"]);
        assert_eq!(wrap_words("one\ntwo", 10), vec!["one", "two"]);
        assert_eq!(wrap_words("short", 80), vec!["short"]);
    }
}
