#!/bin/bash
set -eu
. "$(dirname "${BASH_SOURCE[0]}")/../lib/bench.sh"

sed -i 's/site_action\.process_site_actions"/site_action.process_site_action"/' apps/press/press/hooks.py
grep -q 'site_action\.process_site_action"' apps/press/press/hooks.py
