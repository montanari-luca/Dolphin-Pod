//! Some local servers (LM Studio, llama.cpp, vLLM without a reasoning parser) inline the
//! chain of thought as `<think>…</think>` in the content stream. This splits it back out.

#[derive(Default)]
pub struct ThinkTagSplitter {
    in_think: bool,
    pending: String,
    seen_text: bool,
}

const OPEN: &str = "<think>";
const CLOSE: &str = "</think>";

impl ThinkTagSplitter {
    /// Returns (reasoning, text) produced by this chunk.
    pub fn push(&mut self, chunk: &str) -> (String, String) {
        self.pending.push_str(chunk);
        let (mut reasoning, mut text) = (String::new(), String::new());
        loop {
            let tag = if self.in_think { CLOSE } else { OPEN };
            if let Some(i) = self.pending.find(tag) {
                let before: String = self.pending.drain(..i).collect();
                self.pending.drain(..tag.len());
                if self.in_think {
                    reasoning.push_str(&before);
                } else {
                    // A <think> tag only counts at the start of the answer.
                    if self.seen_text || !before.trim().is_empty() {
                        text.push_str(&before);
                        text.push_str(tag);
                        self.seen_text = true;
                        continue;
                    }
                    text.push_str(&before);
                }
                self.in_think = !self.in_think;
                continue;
            }
            // Keep a possible partial tag at the end for the next chunk.
            let keep = partial_suffix(&self.pending, tag);
            let emit: String = self.pending.drain(..self.pending.len() - keep).collect();
            if self.in_think {
                reasoning.push_str(&emit);
            } else {
                text.push_str(&emit);
            }
            break;
        }
        if !text.trim().is_empty() {
            self.seen_text = true;
        }
        (reasoning, text)
    }

    pub fn finish(&mut self) -> (String, String) {
        let rest = std::mem::take(&mut self.pending);
        if self.in_think { (rest, String::new()) } else { (String::new(), rest) }
    }
}

fn partial_suffix(s: &str, tag: &str) -> usize {
    (1..tag.len()).rev().find(|&n| s.len() >= n && s.is_char_boundary(s.len() - n) && tag.starts_with(&s[s.len() - n..])).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_across_chunks() {
        let mut s = ThinkTagSplitter::default();
        let mut r = String::new();
        let mut t = String::new();
        for c in ["<thi", "nk>plan", " it</th", "ink>\n\nAnswer", " here"] {
            let (a, b) = s.push(c);
            r += &a;
            t += &b;
        }
        let (a, b) = s.finish();
        r += &a;
        t += &b;
        assert_eq!(r, "plan it");
        assert_eq!(t, "\n\nAnswer here");
    }

    #[test]
    fn literal_tag_inside_answer_is_text() {
        let mut s = ThinkTagSplitter::default();
        let (r, t) = s.push("Use the <think> tag like this");
        let (r2, t2) = s.finish();
        assert_eq!(r + &r2, "");
        assert_eq!(t + &t2, "Use the <think> tag like this");
    }
}
