import frappe
from frappe.tests import IntegrationTestCase


class TestLibraryMember(IntegrationTestCase):
	def test_referred_by_email_is_fetched(self):
		member = frappe.get_doc("Library Member", "Ada Lovelace")
		self.assertEqual(member.email_id, "ada@example.com")
