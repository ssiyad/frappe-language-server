// Bound to Library Member by the doctype_js hook, not by directory.
frappe.ui.form.on("Library Member", {
	refresh(frm) {
		frm.add_custom_button(__("Open Loans"), () => {
			frappe.set_route("List", "Library Loan Item", { parent: frm.doc.name });
		});
	},
});

// No `frappe.ui.form.on` of its own: `frm` here is anchored by the
// `doctype_js` hook that bound this file, which is why spec 12 requires
// `hooks.py` to be indexed first. expect: clean
function stamp_membership(frm) {
	frm.set_value("email_id", frm.doc.member_name);
}
