import frappe


def validate_member(doc, method=None):
	if doc.email_id and "@" not in doc.email_id:
		frappe.throw(frappe._("Enter a valid email address"))


def sync_member(doc, method=None):
	frappe.db.set_value("Library Member", doc.name, "referred_by_email", doc.referred_by_email)


def notify_member(doc, method=None):
	pass


def activate_membership(doc, method=None):
	frappe.db.set_value("Library Membership", doc.name, "to_date", doc.to_date)


def audit_change(doc, method=None):
	pass
