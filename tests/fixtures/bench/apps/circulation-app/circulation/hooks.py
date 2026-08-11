# Every deliberate error below carries the code it must produce (specs/23).
# Lines marked `expect: clean` are near misses that MUST stay silent.

app_name = "circulation_app"  # expect: app-name-mismatch (package directory is `circulation`)
app_title = "Circulation"
app_publisher = "frappe-lsp fixtures"
app_description = "Deliberately wrong hooks"
app_email = "fixtures@example.com"
app_license = "gpl-3.0"

required_apps = [
	"library",  # expect: clean
	"acquisitions",  # expect: unknown-required-app
]

doctype_javascript = {"Loan": "public/js/loan.js"}  # expect: unknown-hook

app_include_js = "/assets/circulation/js/missing.js"  # expect: missing-asset

# Binds a script that carries no `frappe.ui.form.on` of its own.
doctype_js = {"Loan": "public/js/loan_helpers.js"}

doc_events = {
	"Loan": {
		"on_validate": "circulation.api.before_loan",  # expect: unknown-doc-event
		# The module exists, so its symbol table is not proof of absence: a
		# re-export is invisible to a reader that sees only top-level
		# definitions (specs/23). expect: clean
		"validate": "circulation.api.no_such_handler",
		# No such module anywhere in the workspace, which is certainly wrong.
		"before_save": "circulation.nowhere.at_all",  # expect: unresolvable-path
		"on_update": "circulation.api.before_loan",  # expect: clean
	},
	"Libary Member": {  # expect: unknown-doctype
		"validate": "circulation.api.before_loan"
	},
	"Library Member": {
		# Library Member is not submittable, so this handler is dead code.
		"on_submit": "circulation.api.before_loan",  # expect: non-submittable-event
	},
	"*": {"on_change": "circulation.api.before_loan"},  # expect: clean
}

# A function where specs/12 requires a class.
override_doctype_class = {"Loan": "circulation.api.mark_returned"}  # expect: wrong-path-kind

scheduler_events = {
	"fortnightly": ["circulation.api.sweep_loans"],  # expect: unknown-frequency
	"daily": ["circulation.api.sweep_loans"],  # expect: clean
	"cron": {
		"0 25 * * *": ["circulation.api.sweep_loans"],  # expect: invalid-cron
		"*/15 * * * *": ["circulation.api.sweep_loans"],  # expect: clean
	},
}

after_install = "circulation.api.setup"
after_install = "circulation.api.setup_again"  # expect: duplicate-hook
