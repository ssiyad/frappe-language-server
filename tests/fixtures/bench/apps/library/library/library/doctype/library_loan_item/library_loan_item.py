import frappe
from frappe.model.document import Document


class LibraryLoanItem(Document):
	def is_overdue(self) -> bool:
		return self.loan_status == "Overdue"
