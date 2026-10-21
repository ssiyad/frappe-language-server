#!/bin/bash
# Writes evals/guided-<case>/ for each case: the same case, with a system prompt
# that points Claude at frappe-lsp. Natural cases measure whether the plugin
# gets used; guided cases measure what it is worth when it does.
set -eu

evals=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
hint="This is a Frappe bench. If the frappe-lsp skill is available, load it first and follow it for DocType, field and hook questions."

rm -rf "$evals"/guided-*
for case in "$evals"/*/prompt.md; do
	source=$(dirname "$case")
	target="$evals/guided-$(basename "$source")"
	cp -r "$source" "$target"
	sed -i "s/^name: .*/name: $(basename "$target")/; s/tags: \[press, natural\]/tags: [press, guided]/" "$target/case.yaml"
	sed -i "0,/^---$/!{0,/^---$/s//append_system_prompt: \"$hint\"\n---/}" "$target/prompt.md"
done
