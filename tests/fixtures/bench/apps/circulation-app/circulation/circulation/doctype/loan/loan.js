frappe.ui.form.on("Loan", {
	refresh(frm) {
		frm.set_value("no_such_field", 1); // expect: unknown-field
		frm.set_value("status", "Closed"); // expect: clean

		frappe.call({
			method: "circulation.api.recalculate_fines", // expect: non-whitelisted-call
			args: { loan: frm.doc.name },
		});
		frappe.call({
			method: "library.api.get_member_summary", // expect: clean
			args: { member: frm.doc.member },
		});
		frappe.call({
			method: "erpnext.accounts.utils.get_balance_on", // expect: clean, resolves nowhere in this workspace
		});

		frappe.msgprint(__("Loan refreshed")); // expect: clean
		frappe.msgprint(__(`Loan ${frm.doc.name} refreshed`)); // expect: translation-literal
	},
	stauts(frm) {}, // expect: unknown-form-event, typo of the `status` fieldname
	status(frm) {}, // expect: clean, a fieldname handler
	items_add(frm) {}, // expect: clean, table event on the `items` field
	onload_post_render(frm) {}, // expect: clean, lifecycle event
});

// A child DocType form script anchors to the child, not to Loan.
frappe.ui.form.on("Library Loan Item", {
	loan_status(frm, cdt, cdn) {
		frappe.model.set_value(cdt, cdn, "book_title", ""); // expect: clean
	},
});
