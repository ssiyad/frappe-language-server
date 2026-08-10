app_name = "library_archive"
app_title = "Library Archive"
app_publisher = "frappe-lsp fixtures"
app_description = "Superseded by `library`. On disk, not in sites/apps.txt."
app_email = "fixtures@example.com"
app_license = "gpl-3.0"

# Both DocTypes below are redefined by `library`. Indexing this app would emit
# two duplicate-doctype diagnostics before the user typed anything — the reason
# specs/03 makes the installed list authoritative.
doc_events = {"Library Member": {"validate": "library_archive.api.validate_member"}}
