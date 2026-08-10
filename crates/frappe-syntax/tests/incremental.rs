//! An incremental `didChange` sequence must yield the same tree as a fresh
//! parse — the invariant [32](../../../docs/specs/32-testing.md) names, and the
//! one that breaks subtly in production and is undebuggable from a bug report.
//!
//! The generator is written out by hand rather than pulled in as a dependency:
//! the input space here is small and a fixed seed is what makes a failure
//! reproducible from the test name alone.

use frappe_syntax::{Buffer, Change, Language, Position, PositionEncoding, Range};

/// Full structure **and** byte ranges. `to_sexp` alone compares shape and would
/// pass on a tree whose ranges never got shifted by the edit.
fn shape(buffer: &Buffer) -> String {
    let mut out = String::new();
    let mut cursor = buffer.tree().walk();
    let mut stack = vec![(cursor.node(), 0usize)];
    while let Some((node, depth)) = stack.pop() {
        out.push_str(&format!(
            "{:indent$}{} {}..{}\n",
            "",
            node.kind(),
            node.start_byte(),
            node.end_byte(),
            indent = depth * 2
        ));
        let children: Vec<_> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev().map(|child| (child, depth + 1)));
    }
    out
}

/// xorshift64*: deterministic, seedable, and short enough to read.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        if bound == 0 {
            0
        } else {
            (self.next() % bound as u64) as usize
        }
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// The position of a byte offset in `text`, in `encoding`. Written separately
/// from the crate's own conversion so the test does not check the code against
/// itself.
fn position(text: &str, offset: usize, encoding: PositionEncoding) -> Position {
    let line = text[..offset].matches('\n').count();
    let line_start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let prefix = &text[line_start..offset];
    let character = match encoding {
        PositionEncoding::Utf8 => prefix.len(),
        PositionEncoding::Utf16 => prefix.chars().map(char::len_utf16).sum(),
    };
    Position::new(line as u32, character as u32)
}

/// A byte offset on a character boundary at or after `offset`.
fn boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

const INSERTS: &[&str] = &[
    "x",
    "\n",
    "\"Sales Invoice\"",
    "frappe.get_doc(",
    ")",
    "# 🧾\n",
    "é",
    "    ",
    "fields=[\"customer\"]",
    "",
];

const PYTHON_SEED: &str = "import frappe\n\
                           \n\
                           \n\
                           def on_submit(self):\n\
                           \tinv = frappe.get_doc(\"Sales Invoice\", self.invoice)\n\
                           \tfrappe.get_all(\"Loan\", fields=[\"status\"], order_by=\"due_date asc\")\n\
                           \treturn inv.customer\n";

const JAVASCRIPT_SEED: &str = "frappe.ui.form.on(\"Task\", {\n\
                               \trefresh(frm) {\n\
                               \t\tfrm.set_value(\"subject\", `🧾 ${frm.doc.name}`);\n\
                               \t},\n\
                               });\n";

/// One randomized edit sequence. Returns the number of edits applied so a
/// failure can name it.
fn run(language: Language, seed_text: &str, seed: u64, encoding: PositionEncoding) {
    let mut rng = Rng(seed | 1);
    let mut reference = seed_text.to_string();
    let mut buffer = Buffer::open(language, seed_text, 1, encoding).expect("a pinned grammar");

    for step in 1..=24 {
        let start = boundary(&reference, rng.below(reference.len() + 1));
        let end = boundary(&reference, start + rng.below(8));
        let text = (*rng.pick(INSERTS)).to_string();

        let change = Change {
            range: Some(Range::new(
                position(&reference, start, encoding),
                position(&reference, end, encoding),
            )),
            text: text.clone(),
        };
        reference.replace_range(start..end, &text);
        buffer.apply(std::slice::from_ref(&change), step + 1);

        assert_eq!(
            buffer.text(),
            reference,
            "seed {seed} step {step}: text diverged"
        );

        let fresh = Buffer::open(language, &reference, 1, encoding).expect("a pinned grammar");
        assert_eq!(
            shape(&buffer),
            shape(&fresh),
            "seed {seed} step {step}: incremental tree diverged from a fresh parse"
        );
    }
}

#[test]
fn an_incremental_edit_sequence_yields_the_same_python_tree_as_a_fresh_parse() {
    for seed in 1..=32 {
        run(Language::Python, PYTHON_SEED, seed, PositionEncoding::Utf16);
    }
}

#[test]
fn an_incremental_edit_sequence_yields_the_same_javascript_tree_as_a_fresh_parse() {
    for seed in 1..=32 {
        run(
            Language::JavaScript,
            JAVASCRIPT_SEED,
            seed,
            PositionEncoding::Utf16,
        );
    }
}

#[test]
fn the_invariant_holds_under_utf8_positions_too() {
    for seed in 1..=16 {
        run(Language::Python, PYTHON_SEED, seed, PositionEncoding::Utf8);
        run(
            Language::JavaScript,
            JAVASCRIPT_SEED,
            seed,
            PositionEncoding::Utf8,
        );
    }
}

#[test]
fn a_batch_of_edits_applied_at_once_matches_a_fresh_parse() {
    // A `didChange` may carry several changes; each range refers to the
    // document as of the previous change in the batch.
    let mut buffer = Buffer::open(Language::Python, PYTHON_SEED, 1, PositionEncoding::Utf16)
        .expect("a pinned grammar");

    buffer.apply(
        &[
            Change {
                range: Some(Range::new(Position::new(4, 24), Position::new(4, 37))),
                text: "Loan".to_string(),
            },
            Change {
                range: Some(Range::new(Position::new(6, 12), Position::new(6, 20))),
                text: "status".to_string(),
            },
        ],
        2,
    );

    let fresh = Buffer::open(Language::Python, &buffer.text(), 1, PositionEncoding::Utf16)
        .expect("a pinned grammar");
    assert_eq!(shape(&buffer), shape(&fresh));
}
