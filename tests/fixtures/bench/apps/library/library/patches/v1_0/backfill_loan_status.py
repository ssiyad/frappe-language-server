import frappe


def execute():
	frappe.db.set_value("Library Loan Item", {"loan_status": ""}, "loan_status", "Issued")
