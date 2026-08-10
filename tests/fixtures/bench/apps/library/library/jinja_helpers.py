import frappe


def member_url(member: str) -> str:
	return f"/books/{frappe.scrub(member)}"


def format_isbn(value: str) -> str:
	digits = "".join(c for c in value if c.isdigit())
	return "-".join([digits[:3], digits[3:4], digits[4:9], digits[9:12], digits[12:]])
