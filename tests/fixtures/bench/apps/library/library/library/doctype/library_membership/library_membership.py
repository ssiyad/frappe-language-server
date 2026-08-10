import frappe
from frappe.model.document import Document


class LibraryMembership(Document):
	def on_submit(self):
		frappe.db.set_value("Library Member", self.member, "referred_by_email", None)
