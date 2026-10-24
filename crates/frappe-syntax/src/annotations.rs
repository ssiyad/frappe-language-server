//! The `# begin: auto-generated types` block `bench` writes into a controller,
//! one `fieldname: DF.<Type>` line per field. Each line names a field of the
//! controller's DocType, so a rename that misses it leaves the types stale
//! ([21](../../../docs/specs/21-navigation.md)).
//!
//! Read as text rather than as a tree: the block is machine-written, one
//! annotation to a line, and what it means depends on the comments around it.

use crate::matches::Span;

const BEGIN: &str = "# begin: auto-generated types";
const END: &str = "# end: auto-generated types";

/// Each fieldname the generated blocks in `source` annotate, with its span.
#[must_use]
pub fn generated_fields(source: &str) -> Vec<(&str, Span)> {
    let mut found = Vec::new();
    let mut rest = 0;
    while let Some(at) = source[rest..].find(BEGIN) {
        let begin = rest + at;
        let end = source[begin..]
            .find(END)
            .map_or(source.len(), |at| begin + at);
        annotations_in(source, begin, end, &mut found);
        rest = end;
    }
    found
}

fn annotations_in<'a>(source: &'a str, begin: usize, end: usize, found: &mut Vec<(&'a str, Span)>) {
    let mut offset = begin;
    for line in source[begin..end].split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some((name, kind)) = trimmed.split_once(':')
            && kind.trim_start().starts_with("DF.")
            && is_identifier(name)
        {
            let start = offset + (line.len() - trimmed.len());
            found.push((name, Span::new(start, start + name.len())));
        }
        offset += line.len();
    }
}

fn is_identifier(text: &str) -> bool {
    text.chars()
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && text.chars().all(|c| c.is_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONTROLLER: &str = "class Loan(Document):
\t# begin: auto-generated types
\tfrom typing import TYPE_CHECKING

\tif TYPE_CHECKING:
\t\tfrom frappe.types import DF

\t\tstatus: DF.Literal[\"Open\", \"Closed\"]
\t\tmember: DF.Link | None
\t# end: auto-generated types

\tdef validate(self):
\t\tlabel: DF.Data = self.member
";

    #[test]
    fn each_annotation_in_the_block_is_a_field_at_its_name() {
        let found = generated_fields(CONTROLLER);
        let names: Vec<&str> = found.iter().map(|(name, _)| *name).collect();
        assert_eq!(names, ["status", "member"]);
        for (name, span) in found {
            assert_eq!(&CONTROLLER[span.range()], name);
        }
    }

    #[test]
    fn an_annotation_outside_the_block_is_not_one() {
        assert!(generated_fields("def f():\n\tlabel: DF.Data = x\n").is_empty());
    }

    #[test]
    fn a_block_with_no_end_runs_to_the_end_of_the_file() {
        let found = generated_fields("# begin: auto-generated types\n\tamount: DF.Currency\n");
        assert_eq!(found.len(), 1);
    }
}
