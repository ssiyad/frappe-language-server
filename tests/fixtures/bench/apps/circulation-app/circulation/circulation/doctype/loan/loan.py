import frappe
from frappe.model.document import Document


class Loan(Document):
	def validate(self):
		if not self.items:
			frappe.throw(frappe._("A loan needs at least one item"))
