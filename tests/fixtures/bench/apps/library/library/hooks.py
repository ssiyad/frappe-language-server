app_name = "library"
app_title = "Library"
app_publisher = "frappe-lsp fixtures"
app_description = "Every hook value schema in specs/12, all of it resolvable"
app_email = "fixtures@example.com"
app_license = "gpl-3.0"

# [AppName]
required_apps = []

# File | [File]
app_include_js = "/assets/library/js/library.js"
app_include_css = ["/assets/library/css/library.css"]
web_include_js = ["/assets/library/js/library.js"]
web_include_css = "/assets/library/css/library.css"

# Map<DocTypeName, File | [File]>
doctype_js = {"Library Member": "public/js/library_member.js"}
doctype_list_js = {"Library Member": "public/js/library_member_list.js"}
doctype_tree_js = {"Library Member": "public/js/library_member_tree.js"}
doctype_calendar_js = {"Library Membership": "public/js/library_membership_calendar.js"}

# Map<DocTypeName, Map<DocEvent, Path(Callable) | [Path]>>
doc_events = {
	"Library Member": {
		"validate": "library.events.validate_member",
		"on_update": [
			"library.events.sync_member",
			"library.events.notify_member",
		],
	},
	"Library Membership": {
		# on_submit is legal here: Library Membership is submittable.
		"on_submit": "library.events.activate_membership",
		# Path(Method) form — a method on a resolved class.
		"on_cancel": "library.overrides.MembershipMixin.recalculate",
	},
	# "*" applies to every DocType and is not a DocType name.
	"*": {"on_change": "library.events.audit_change"},
}

# Map<DocTypeName, Path(Class)>
override_doctype_class = {"Library Member": "library.overrides.CustomLibraryMember"}

# Map<DocTypeName, [Path(Class)]>
extend_doctype_class = {"Library Membership": ["library.overrides.MembershipMixin"]}

# Map<DocTypeName, Path(Callable)>
permission_query_conditions = {"Library Member": "library.permissions.member_query_conditions"}
has_permission = {"Library Member": "library.permissions.has_member_permission"}

# Map<Path(Whitelisted), Path(Callable)>
override_whitelisted_methods = {"library.api.get_member_summary": "library.api.get_member_summary_v2"}

# Map<Frequency, [Path(Callable)]>, plus the cron sub-map
scheduler_events = {
	"daily": ["library.tasks.send_due_reminders"],
	"hourly_long": ["library.tasks.rebuild_availability"],
	"cron": {"0 3 * * *": ["library.tasks.archive_old_loans"]},
}

# [DocTypeName | {dt, filters}]
fixtures = [
	"Library Loan Item",
	{"dt": "Library Member", "filters": [["member_name", "like", "A%"]]},
]

# {methods: [Path(Module)], filters: [Path(Callable)]}
jinja = {
	"methods": ["library.jinja_helpers"],
	"filters": ["library.jinja_helpers.format_isbn"],
}

# [{from_route, to_route}]
website_route_rules = [{"from_route": "/books/<book_id>", "to_route": "books"}]

# Path(Callable) — lifecycle
before_install = "library.install.before_install"
after_install = "library.install.after_install"
after_migrate = "library.install.after_migrate"
on_login = "library.auth.on_login"

# Str — opaque, no resolution
website_context = {"brand_html": "Library"}

# Present but opaque: the value is not a literal, so no value validation runs
# (specs/12). The key is still known to be set.
_QUERY_CONDITIONS = "library.permissions.member_query_conditions"
standard_queries = {doctype: _QUERY_CONDITIONS for doctype in ("Library Member",)}
