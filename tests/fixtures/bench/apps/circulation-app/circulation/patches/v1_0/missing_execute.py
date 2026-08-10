import frappe


# A patch module without execute() is never run and never reports why.
def run():
	frappe.db.sql("select 1")
