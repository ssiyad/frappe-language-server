# Copyright (c) 2026, frappe-lsp fixtures and contributors
# For license information, please see license.txt

import frappe
from frappe.model.document import Document


class LibraryMember(Document):
	def validate(self):
		self.referred_by_email = frappe.db.get_value("Library Member", self.referred_by, "email_id")

	def add_loan(self, title: str, due_date: str) -> None:
		self.append("loans", {"book_title": title, "due_date": due_date, "loan_status": "Issued"})

	@frappe.whitelist()
	def renew_all(self) -> int:
		for row in self.loans:
			row.loan_status = "Issued"
		self.save()
		return len(self.loans)
