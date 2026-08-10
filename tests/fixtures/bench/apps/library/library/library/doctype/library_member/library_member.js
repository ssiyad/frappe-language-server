// Copyright (c) 2026, frappe-lsp fixtures and contributors
// For license information, please see license.txt

frappe.ui.form.on("Library Member", {
	refresh(frm) {
		frm.toggle_display("referred_by_email", Boolean(frm.doc.referred_by));
	},
	member_name(frm) {
		frm.set_value("email_id", `${frappe.scrub(frm.doc.member_name)}@example.com`);
	},
	loans_add(frm) {
		frm.refresh_field("loans");
	},
	validate(frm) {
		frappe.call({
			method: "library.api.get_member_summary",
			args: { member: frm.doc.name },
		});
	},
});

frappe.ui.form.on("Library Loan Item", {
	loan_status(frm, cdt, cdn) {
		frappe.model.set_value(cdt, cdn, "due_date", frappe.datetime.add_days(frappe.datetime.nowdate(), 14));
	},
});
