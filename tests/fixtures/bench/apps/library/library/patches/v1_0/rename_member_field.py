import frappe


def execute():
	frappe.reload_doc("library", "doctype", "library_member")
