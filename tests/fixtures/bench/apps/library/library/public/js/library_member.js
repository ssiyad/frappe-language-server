// Bound to Library Member by the doctype_js hook, not by directory.
frappe.ui.form.on("Library Member", {
	refresh(frm) {
		frm.add_custom_button(__("Open Loans"), () => {
			frappe.set_route("List", "Library Loan Item", { parent: frm.doc.name });
		});
	},
});
