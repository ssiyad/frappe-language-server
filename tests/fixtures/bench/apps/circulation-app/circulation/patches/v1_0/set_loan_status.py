import frappe


def execute():
	frappe.db.set_value("Loan", {"status": ""}, "status", "Open")
