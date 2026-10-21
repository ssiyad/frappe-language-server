#!/bin/bash
set -eu
. "$(dirname "${BASH_SOURCE[0]}")/../lib/bench.sh"

site=apps/press/press/api/site.py
sed -i 's/job\.step_job if job else None/getattr(job, "step_job", None)/' "$site"
sed -i 's/version_upgrade\.site_update, "update_job")/version_upgrade.site_update, "agent_job")/' "$site"
git -C apps/press diff > change.diff
[ "$(grep -c '^[-+][^-+]' change.diff)" = 4 ]
