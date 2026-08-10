import frappe


def member_query_conditions(user: str | None = None) -> str:
	if not user:
		user = frappe.session.user
	return f"`tabLibrary Member`.owner = {frappe.db.escape(user)}"


def has_member_permission(doc, ptype=None, user=None) -> bool:
	return True
