# Copyright (c) 2026, frappe-lsp fixtures and contributors
# For license information, please see license.txt

import frappe
from frappe.model.document import Document

from library.mixins import MembershipMixin


# Not a controller. Its `self` is not a document, so none of the attributes
# below is a field claim.
class LoanTally:

	def __init__(self, rows) -> None:
		self.rows = rows
		self.overdue_count = 0

	def count(self) -> int:
		return len(self.rows) + self.overdue_count


class LibraryMember(Document, MembershipMixin):
	def validate(self):
		self.referred_by_email = frappe.db.get_value("Library Member", self.referred_by, "email_id")

	def summarise(self) -> str:
		# Working state, not fields. Both are assigned above or on the mixin,
		# and reading them back must stay silent. expect: clean
		self._loan_tally = LoanTally(self.loans)
		return f"{self._loan_tally.count()} / {self.membership_label}"

	def after_insert(self):
		# Not a field: handed in through `frappe.get_doc({...})`, which copies
		# every key onto the document.
		if hasattr(self, "welcome_note") and self.welcome_note:
			frappe.msgprint(self.welcome_note)

	def add_loan(self, title: str, due_date: str) -> None:
		self.append("loans", {"book_title": title, "due_date": due_date, "loan_status": "Issued"})

	@frappe.whitelist()
	def renew_all(self) -> int:
		for row in self.loans:
			row.loan_status = "Issued"
		self.save()
		return len(self.loans)
