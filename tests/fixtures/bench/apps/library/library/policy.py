import frappe


def overdue(loan):
	"""Read through the call form: `get_hooks("key")`."""
	return frappe.get_hooks("loan_policies")


def greet(member):
	"""Read through the binding form: frappe returns a _dict and reads
	attributes off it, so the local has to be followed (specs/12)."""
	hooks = frappe.get_hooks()
	return hooks.member_greeters
