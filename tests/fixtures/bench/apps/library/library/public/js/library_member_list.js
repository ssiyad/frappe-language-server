frappe.listview_settings["Library Member"] = {
	add_fields: ["member_name", "email_id"],
	get_indicator(doc) {
		return [__("Member"), "blue", "member_name,=," + doc.member_name];
	},
};
