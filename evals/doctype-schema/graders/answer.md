---
type: llm
---

The correct answer: Site Update's `difference` field links to Deploy Candidate Difference. Its Link fields are `group` (Release Group), `source` (Deploy Candidate), `destination` (Deploy Candidate) and `team` (Team). Its title field is `group`.

PASS if the response names Deploy Candidate Difference, lists all four Link fields with the correct targets, and gives `group` as the title field. Listing `apps` as a Table field is fine.
FAIL if a Link field is missing, a target is wrong, a field that is not a Link is listed as one, or the title field is wrong or missing.
