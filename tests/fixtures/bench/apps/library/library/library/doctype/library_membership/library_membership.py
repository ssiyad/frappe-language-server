import frappe
from frappe.model.document import Document


class LibraryMembership(Document):
	# begin: auto-generated types
	# This code is auto-generated. Do not modify anything in this block.

	from typing import TYPE_CHECKING

	if TYPE_CHECKING:
		from frappe.types import DF

		amended_from: DF.Link | None
		amount: DF.Currency
		currency: DF.Link | None
		from_date: DF.Date
		member: DF.Link
		to_date: DF.Date
	# end: auto-generated types

	def on_submit(self):
		frappe.db.set_value("Library Member", self.member, "referred_by_email", None)
