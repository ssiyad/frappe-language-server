/// Frappe's `scrub`: `Sales Order` becomes `sales_order`.
///
/// ```
/// # use frappe_index::scrub;
/// assert_eq!(scrub("Sales Order"), "sales_order");
/// assert_eq!(scrub("Item-Group"), "item_group");
/// ```
///
/// Source of truth is `frappe/utils/data.py`:
///
/// ```python
/// def scrub(txt: str) -> str:
///     return cstr(txt).replace(" ", "_").replace("-", "_").lower()
/// ```
///
/// It is lossy — `Item-Group` and `Item Group` both scrub to `item_group` — so
/// path to name resolution goes through the JSON `name` key, never back through
/// an unscrub ([04](../../../docs/specs/04-metadata-index.md)).
#[must_use]
pub fn scrub(text: &str) -> String {
    text.chars()
        .map(|c| if c == ' ' || c == '-' { '_' } else { c })
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::scrub;

    #[test]
    fn matches_frappes_implementation() {
        assert_eq!(scrub("DocType"), "doctype");
        assert_eq!(scrub("Sales Invoice"), "sales_invoice");
        assert_eq!(scrub("Item-Group"), "item_group");
        assert_eq!(scrub("Library Loan Item"), "library_loan_item");
        assert_eq!(scrub("UOM"), "uom");
        assert_eq!(scrub(""), "");
    }

    #[test]
    fn is_lossy_in_the_documented_way() {
        assert_eq!(scrub("Item-Group"), scrub("Item Group"));
    }

    #[test]
    fn is_idempotent() {
        for name in [
            "Sales Order",
            "item_group",
            "Web Form",
            "UOM Conversion-Factor",
        ] {
            assert_eq!(scrub(&scrub(name)), scrub(name));
        }
    }
}
