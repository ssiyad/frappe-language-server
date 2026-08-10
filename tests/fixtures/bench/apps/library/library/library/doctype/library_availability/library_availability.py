from frappe.model.document import Document


class LibraryAvailability(Document):
	# A virtual DocType has no table: every persistence method is the app's job.
	def db_insert(self, *args, **kwargs):
		pass

	def load_from_db(self):
		pass

	def db_update(self, *args, **kwargs):
		pass

	def delete(self, *args, **kwargs):
		pass

	@staticmethod
	def get_list(filters=None, page_length=20, **kwargs):
		return []

	@staticmethod
	def get_count(filters=None, **kwargs):
		return 0
