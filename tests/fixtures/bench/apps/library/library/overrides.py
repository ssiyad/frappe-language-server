from library.library.doctype.library_member.library_member import LibraryMember


class CustomLibraryMember(LibraryMember):
	def validate(self):
		super().validate()


class MembershipMixin:
	def recalculate(self):
		self.db_set("amount", self.amount or 0)
