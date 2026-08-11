// Bound to Loan by `doctype_js`, with no `frappe.ui.form.on` of its own. The
// hook is the only thing that says which form `frm` belongs to (specs/12), so
// this file is the positive case for that anchoring: without it, `frm` is
// unbound and the wrong fieldname below reports nothing at all.
function recalculate(frm) {
	frm.set_value("no_such_field", 1); // expect: unknown-field
	frm.set_value("status", "Closed"); // expect: clean
}
