frappe.treeview_settings["Library Member"] = {
	get_tree_nodes: "library.api.list_issued_titles",
	fields: [{ fieldtype: "Data", fieldname: "member_name", label: __("Member Name") }],
};
