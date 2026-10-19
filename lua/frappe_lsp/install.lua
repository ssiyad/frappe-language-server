-- The server binary: where it is, and fetching a release of it.

local M = {}

M.repository = 'ssiyad/frappe-language-server'

--- Where `:FrappeLspInstall` puts the binary.
function M.path()
  return vim.fs.joinpath(vim.fn.stdpath('data'), 'frappe-lsp', 'frappe-lsp')
end

--- The command to run: `vim.g.frappe_lsp_cmd`, else an installed release,
--- else `frappe-lsp` on PATH.
function M.binary()
  if vim.g.frappe_lsp_cmd then
    return vim.g.frappe_lsp_cmd
  end
  if vim.fn.executable(M.path()) == 1 then
    return M.path()
  end
  return 'frappe-lsp'
end

--- The release target for this machine, or nil where there is none.
function M.target()
  local uname = vim.uv.os_uname()
  local arch = ({ x86_64 = 'x86_64', amd64 = 'x86_64', aarch64 = 'aarch64', arm64 = 'aarch64' })[uname.machine]
  local os = ({ Linux = 'unknown-linux-musl', Darwin = 'apple-darwin' })[uname.sysname]
  if arch and os then
    return arch .. '-' .. os
  end
end

-- Download, check against SHA256SUMS, unpack, and replace the binary in one
-- move, so a failed download never leaves a broken one behind.
local script = [[
set -eu
dir=$1 tag=$2 target=$3 repo=$4
# An authenticated gh reaches a private repository; curl needs a public one.
if command -v gh >/dev/null 2>&1 && gh auth status >/dev/null 2>&1; then
  via=gh
else
  via=curl
fi
if [ -z "$tag" ]; then
  if [ "$via" = gh ]; then
    tag=$(gh release view --repo "$repo" --json tagName --jq .tagName)
  else
    tag=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" |
      sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)
  fi
  [ -n "$tag" ] || { echo "no release found for $repo" >&2; exit 1; }
fi
name="frappe-lsp-$tag-$target"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"
if [ "$via" = gh ]; then
  gh release download "$tag" --repo "$repo" --pattern "$name.tar.gz" --pattern SHA256SUMS
else
  base="https://github.com/$repo/releases/download/$tag"
  curl -fsSL -o "$name.tar.gz" "$base/$name.tar.gz"
  curl -fsSL -o SHA256SUMS "$base/SHA256SUMS"
fi
grep " $name.tar.gz\$" SHA256SUMS > expected
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum -c expected >/dev/null
else
  shasum -a 256 -c expected >/dev/null
fi
tar xzf "$name.tar.gz"
mkdir -p "$dir"
cp "$name/frappe-lsp" "$dir/frappe-lsp.new"
chmod +x "$dir/frappe-lsp.new"
mv -f "$dir/frappe-lsp.new" "$dir/frappe-lsp"
echo "$tag"
]]

--- Install a release, the latest when `tag` is empty. `done(err, tag)` runs on
--- the main loop.
function M.install(tag, done)
  local target = M.target()
  if not target then
    local uname = vim.uv.os_uname()
    return done(('no release for %s %s; build it with cargo'):format(uname.sysname, uname.machine))
  end
  local dir = vim.fs.dirname(M.path())
  vim.system({ 'sh', '-c', script, 'install', dir, tag or '', target, M.repository }, { text = true }, function(result)
    vim.schedule(function()
      if result.code ~= 0 then
        done(vim.trim(result.stderr ~= '' and result.stderr or result.stdout))
      else
        done(nil, vim.trim(result.stdout))
      end
    end)
  end)
end

return M
