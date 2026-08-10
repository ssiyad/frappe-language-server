import frappe


def send_due_reminders():
	for loan in frappe.get_all("Library Loan Item", filters={"loan_status": "Overdue"}, pluck="book_title"):
		frappe.logger().info(f"overdue: {loan}")


def rebuild_availability():
	pass


def archive_old_loans():
	pass
