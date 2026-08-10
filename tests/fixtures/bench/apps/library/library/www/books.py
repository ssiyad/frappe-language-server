import frappe


def get_context(context):
	context.loans = frappe.get_all("Library Loan Item", fields=["book_title", "due_date"])
	return context
