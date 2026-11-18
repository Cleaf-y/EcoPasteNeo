#!/bin/bash
# Local native-app bundle. Existing Tauri packaging remains independent.
set -euo pipefail
repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_dir"
build=true
test_data_parent=""
while [[ $# -gt 0 ]]; do
case "$1" in
  "") ;;
  --no-build) build=false; shift ;;
  --test-data-parent) test_data_parent="${2:?Missing test data parent}"; shift 2 ;;
  *) printf 'Usage: %s [--no-build] [--test-data-parent PATH]\n' "$0" >&2; exit 2 ;;
esac
done
if [[ $# -ne 0 || "$(uname -s)" != Darwin ]]; then
  printf 'Run on macOS: %s [--no-build] [--test-data-parent PATH]\n' "$0" >&2; exit 2
fi
if [[ "$build" == true ]]; then
  cargo build -p ecopaste-neo-app --bin EcoPasteNeo --locked --target-dir "$repo_dir/target"
fi
binary="$repo_dir/target/debug/EcoPasteNeo"
bundle="$repo_dir/dist/EcoPasteNeo.app"
if [[ ! -x "$binary" ]]; then
  printf 'Missing executable: %s\n' "$binary" >&2; exit 1
fi
mkdir -p "$bundle/Contents/MacOS"
cp "$binary" "$bundle/Contents/MacOS/EcoPasteNeo"
chmod 755 "$bundle/Contents/MacOS/EcoPasteNeo"
# Read the authoritative identity constants; the bundle must match Storage.
python3 - "$repo_dir" "$bundle" "$test_data_parent" <<'PY'
from pathlib import Path
import plistlib, re, sys, tomllib
repo, bundle = map(Path, sys.argv[1:3])
test_parent = sys.argv[3]
identity = (repo / 'crates/core/src/identity.rs').read_text()
def constant(name):
    return re.search(rf'pub const {name}: &str = "([^"]+)";', identity).group(1)
name = constant('PRODUCT_NAME')
version = tomllib.loads((repo / 'crates/app/Cargo.toml').read_text())['package']['version']
info = {
    'CFBundleDevelopmentRegion': 'zh_CN',
    'CFBundleLocalizations': ['zh_CN', 'en'],
    'CFBundleExecutable': name,
    'CFBundleIdentifier': constant('APPLICATION_ID'),
    'CFBundleInfoDictionaryVersion': '6.0',
    'CFBundleName': name,
    'CFBundleDisplayName': name,
    'CFBundlePackageType': 'APPL',
    'CFBundleShortVersionString': version,
    'CFBundleVersion': version,
    'LSUIElement': True,
    'NSHighResolutionCapable': True,
}
if test_parent:
    if not Path(test_parent).is_absolute():
        raise SystemExit('Test data parent must be absolute')
    info['LSEnvironment'] = {'ECOPASTENEO_DATA_PARENT': test_parent, 'ECOPASTENEO_SHOW': '1'}
with (bundle / 'Contents/Info.plist').open('wb') as file:
    plistlib.dump(info, file)
PY
codesign --force --sign - "$bundle"
printf 'Packaged: %s\n' "$bundle"
