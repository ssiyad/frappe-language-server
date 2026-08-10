import frappe
from frappe import whitelist


@frappe.whitelist()
def get_member_summary(member: str) -> dict:
	doc = frappe.get_doc("Library Member", member)
	return {"name": doc.member_name, "email": doc.email_id, "loans": len(doc.loans)}


@frappe.whitelist()
def get_member_summary_v2(member: str) -> dict:
	summary = get_member_summary(member)
	summary["loyalty_points"] = frappe.db.get_value("Library Member", member, "loyalty_points")
	return summary


@frappe.whitelist(allow_guest=True, methods=["GET"])
def list_issued_titles() -> list[dict]:
	return frappe.get_all(
		"Library Loan Item",
		fields=["book_title", "due_date"],
		filters={"loan_status": "Issued"},
		order_by="due_date asc",
	)


# Recognized per specs/07: whitelist imported from frappe, used unqualified.
@whitelist()
def ping() -> str:
	return "pong"


def loan_period() -> int:
	return frappe.db.get_single_value("Library Settings", "loan_period")


def open_loans_for(member: str) -> int:
	return frappe.db.count("Library Loan Item", {"loan_status": "Issued", "parent": member})
