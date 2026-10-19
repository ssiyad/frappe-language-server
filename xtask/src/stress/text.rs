//! Where to ask, and what to type: positions and edits over one document.

use serde_json::{Value, json};

/// Fragments an editor sees mid-keystroke: open strings, open calls, a dot with
/// nothing after it. These are the states a server meets most and that a
/// corpus of finished code never shows it.
const FRAGMENTS: [&str; 14] = [
    "\"",
    "'",
    ".",
    "(",
    "{",
    "frappe.get_doc(\"",
    "frappe.db.get_value(\"Item\", {\"",
    "frm.doc.",
    "frm.set_value(\"",
    "doc.",
    "self.",
    "__(\"",
    "\n\t",
    "é🙂",
];

/// A deterministic generator, so a failing run can be replayed by its seed.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1))
    }

    pub fn below(&mut self, bound: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        if bound == 0 {
            0
        } else {
            usize::try_from(self.0 >> 33).unwrap_or(0) % bound
        }
    }
}

/// Byte offsets worth asking about: the start and middle of every name, and
/// the position right after a quote, a dot or an open paren.
pub fn candidates(source: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut run_start = None;
    let mut previous = '\0';
    for (offset, c) in source.char_indices() {
        let is_name = c.is_alphanumeric() || c == '_';
        match (is_name, run_start) {
            (true, None) => run_start = Some(offset),
            (false, Some(start)) => {
                found.push(start);
                found.push(floor(source, start + (offset - start) / 2));
                run_start = None;
            }
            _ => {}
        }
        if matches!(previous, '"' | '\'' | '.' | '(') {
            found.push(offset);
        }
        previous = c;
    }
    found.push(source.len());
    found.sort_unstable();
    found.dedup();
    found
}

/// At most `count` of `all`, spread evenly so a long file is not only its head.
pub fn sample(all: &[usize], count: usize, rng: &mut Rng) -> Vec<usize> {
    if all.len() <= count {
        return all.to_vec();
    }
    let stride = all.len() / count;
    (0..count)
        .map(|index| all[index * stride + rng.below(stride)])
        .collect()
}

/// An edit as LSP sends it, plus the text it leaves behind.
pub struct Edit {
    pub change: Value,
    pub text: String,
    /// Where the cursor sits afterwards.
    pub cursor: usize,
}

/// Insert a fragment, or delete a short span, somewhere in `source`.
pub fn edit(source: &str, at: usize, rng: &mut Rng) -> Edit {
    let at = floor(source, at.min(source.len()));
    if rng.below(3) == 0 && at < source.len() {
        let end = floor(source, (at + 1 + rng.below(40)).min(source.len()));
        let end = if end == at { source.len() } else { end };
        let mut text = source.to_string();
        text.replace_range(at..end, "");
        return Edit {
            change: json!({"range": range(source, at, end), "text": ""}),
            text,
            cursor: at,
        };
    }
    let fragment = FRAGMENTS[rng.below(FRAGMENTS.len())];
    let mut text = source.to_string();
    text.insert_str(at, fragment);
    Edit {
        change: json!({"range": range(source, at, at), "text": fragment}),
        text,
        cursor: at + fragment.len(),
    }
}

/// The LSP position of a byte offset, in UTF-16 code units.
pub fn position(source: &str, offset: usize) -> Value {
    let before = &source[..floor(source, offset)];
    let line = before.matches('\n').count();
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    let character: usize = before[line_start..].chars().map(char::len_utf16).sum();
    json!({"line": line, "character": character})
}

pub fn range(source: &str, start: usize, end: usize) -> Value {
    json!({"start": position(source, start), "end": position(source, end)})
}

/// The nearest char boundary at or below `offset`.
fn floor(source: &str, mut offset: usize) -> usize {
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}
