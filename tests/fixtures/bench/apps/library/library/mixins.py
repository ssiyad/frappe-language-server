# Copyright (c) 2026, frappe-lsp fixtures and contributors
# For license information, please see license.txt


class MembershipMixin:
	"""A base class the controller derives from. What it hangs off `self` is
	the controller's state too, so `self.membership_label` on Library Member
	must stay silent (specs/04)."""

	def label_membership(self) -> None:
		self.membership_label = "member"
