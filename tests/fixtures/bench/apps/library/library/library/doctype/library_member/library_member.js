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
	// A grid event is registered against the child, with the *parent's* table
	// fieldname: `grid.js` triggers it as `(fieldname + "_add", row.doctype)`.
	// expect: clean
	loans_add(frm, cdt, cdn) {
		// `frm` is the parent's form even in a child's block, and this child
		// has two parents, Library Member and Loan. Which one is not knowable,
		// so nothing is said, not even about a name neither parent declares.
		frm.refresh_field("loans"); // expect: clean
		frm.set_value("email_id", frm.doc.member_name); // expect: clean
		frm.refresh_field("loan_items"); // expect: clean
	},
});

frappe.ui.form.on("Library Member", {
	// Neither an event nor a fieldname, but the handler below calls it by
	// name, which is all a handler needs to fire. expect: clean
	refresh_membership_banner(frm) {
		frm.set_intro(__("Membership"));
	},
	onload(frm) {
		frm.trigger("refresh_membership_banner");
		frm.events.stamp_visit(frm);
		// `locals[":DocType"]` is frappe's own notation for a Single's cached
		// record; the DocType is what follows the colon. expect: clean
		const settings = locals[":Library Settings"];
		return settings;
	},
	stamp_visit(frm) {
		frm.set_value("email_id", frm.doc.email_id);
	},
});

// The legacy three-argument form takes an *event*, not a fieldname: `refresh`
// here is a lifecycle event and must not be read as a missing field.
// expect: clean
frappe.ui.form.on("Library Member", "refresh", function (frm) {
	frm.add_custom_button(__("Renew"), () => frm.trigger("refresh_membership_banner"));
});
