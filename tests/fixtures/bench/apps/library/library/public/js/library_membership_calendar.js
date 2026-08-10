frappe.views.calendar["Library Membership"] = {
	field_map: {
		start: "from_date",
		end: "to_date",
		id: "name",
		title: "member",
	},
	get_events_method: "library.api.list_issued_titles",
};
