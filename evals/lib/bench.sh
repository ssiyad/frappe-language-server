# Builds a bench in the run's empty workspace, so frappe-lsp has a root.
# Sourced by each case's scaffold.sh. Needs evals/.bench: the path of a
# local bench whose apps hold the pinned commits below.

frappe_commit=ec31833020b70f3d4a897b51595f39f9a0f300c5
press_commit=1904e38d3d80a640e9a11cfd92954fcc87084817

evals=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
real_home=$(getent passwd "$(id -u)" | cut -d: -f6)

if [ ! -f "$evals/.bench" ]; then
	echo "Write the path of a local bench to $evals/.bench" >&2
	exit 1
fi
sources=$(cat "$evals/.bench")

# An installed plugin has its bin/ on the Bash PATH; `claude plugin eval`
# loads it inline and does not add it, so the skill's `frappe-lsp` is not
# found. The run inherits PATH from the eval, so the eval must set it. Any
# other frappe-lsp on PATH, such as one under the real home, is not readable
# from the sandbox.
launcher=$(cd "$evals/../bin" && pwd)/frappe-lsp
if [ "$(command -v frappe-lsp)" != "$launcher" ]; then
	echo "Put $(dirname "$launcher") first on PATH: see evals/README.md" >&2
	exit 1
fi

clone() {
	git clone -q --shared --no-checkout "file://$sources/apps/$1" "apps/$1"
	git -C "apps/$1" checkout -q "$2"
}

mkdir -p apps sites
clone frappe "$frappe_commit"
clone press "$press_commit"
printf 'frappe\npress\n' > sites/apps.txt

# The run's HOME is empty; reuse the server bin/frappe-lsp downloaded already,
# or the local build evals/.server names. A copy, not a link: a Bash grant
# sandboxes the run, and the sandbox cannot read the real home.
version=$(sed -n 's/.*"version": "\(.*\)".*/\1/p' "$evals/../package.json")
server="$real_home/.local/share/frappe-lsp/$version/frappe-lsp"
[ -f "$evals/.server" ] && server=$(cat "$evals/.server")
mkdir -p "$HOME/.local/share/frappe-lsp/$version"
cp "$server" "$HOME/.local/share/frappe-lsp/$version/frappe-lsp"

python3 -I "$evals/lib/warm.py" "$evals/../bin/frappe-lsp"
