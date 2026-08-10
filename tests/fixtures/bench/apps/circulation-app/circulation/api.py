import frappe
from frappe import _

# Hook targets. `mark_returned` is referenced by override_doctype_class, which
# requires a class — the point of the wrong-path-kind case in hooks.py.


def before_loan(doc, method=None):
	pass


def mark_returned(loan: str) -> None:
	frappe.db.set_value("Loan", loan, "status", "Closed")
	doc = frappe.get_doc("Loan", loan)
	# Attribute access, off by default: a Document is an open Python object, so
	# this is a claim only when the user asks for it (specs/23).
	print(doc.no_such_attribute)  # expect: unknown-attribute
	print(doc.status)  # expect: clean


def sweep_loans():
	pass


def setup():
	pass


def setup_again():
	pass


# Not whitelisted: loan.js calling this is a runtime PermissionError.
def recalculate_fines(loan: str) -> float:
	return 0.0


# --- DocType and fieldname positions ----------------------------------------


def member_email(member: str) -> str | None:
	return frappe.db.get_value("Libary Member", member, "email_id")  # expect: unknown-doctype


def member_emails(member: str) -> list[str]:
	return frappe.get_all(
		"Library Member",
		filters={"member_name": member},
		pluck="emial_id",  # expect: unknown-field
	)


def loan_period() -> int:
	# Library Member is not a Single.
	return frappe.db.get_single_value("Library Member", "member_name")  # expect: not-single-doctype


def child_member_name(row: str) -> str:
	child = frappe.get_doc("Library Loan Item", row)
	return child.member_name  # expect: parent-field-on-child


# --- Near misses: every line below MUST stay silent --------------------------

MEMBER_DOCTYPE = "Library Member"


def dynamic_doctype(doctype: str, name: str):
	return frappe.get_doc(doctype, name)  # expect: clean, not a literal


def constant_doctype(name: str):
	return frappe.get_doc(MEMBER_DOCTYPE, name)  # expect: clean, no cross-function inference


def raw_sql():
	return frappe.db.sql("select emial_id from `tabLibrary Member`")  # expect: clean, specs/10 excludes SQL


def aggregate():
	return frappe.get_all("Library Member", fields=["count(name) as total"])  # expect: clean


def dotted_field():
	return frappe.get_all("Library Member", fields=["referred_by.member_name"])  # expect: clean


def aliased_import(name: str):
	import frappe as f

	return f.get_doc("Libary Member", name)  # expect: clean, `import frappe as f` is not recognized


def four_tuple_filters():
	return frappe.get_all(
		"Library Member",
		filters=[["Library Loan Item", "loan_status", "=", "Issued"]],  # expect: clean, re-anchors to its own DocType
	)


# --- Translations ------------------------------------------------------------


def messages(name: str) -> list[str]:
	return [
		_("Loan created"),  # expect: clean
		_("Loan for {0} created").format(name),  # expect: clean, format applied outside
		_(f"Loan for {name} created"),  # expect: translation-literal
		_("Loan for {0} created".format(name)),  # expect: translation-literal
		_("Loan for " + name + " created"),  # expect: translation-literal
		_(name),  # expect: translation-dynamic
	]
