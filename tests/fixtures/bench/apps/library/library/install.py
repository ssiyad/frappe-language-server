import frappe


def before_install():
	pass


def after_install():
	settings = frappe.get_single("Library Settings")
	settings.loan_period = 14
	settings.save()


def after_migrate():
	pass
