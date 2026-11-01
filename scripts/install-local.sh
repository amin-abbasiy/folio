#!/bin/sh
set -eu

# Installs only for the current user. Never uses sudo or touches host services.
cd "$(dirname "$0")/.."
cargo build --release --locked
prefix="${HOME}/.local"
install -Dm755 target/release/folio "$prefix/bin/folio"
install -Dm644 assets/dev.folio.Notes.desktop "$prefix/share/applications/dev.folio.Notes.desktop"
install -Dm644 assets/dev.folio.Notes.svg "$prefix/share/icons/hicolor/scalable/apps/dev.folio.Notes.svg"
install -Dm644 LICENSE "$prefix/share/doc/folio/LICENSE"
install -Dm644 assets/INTER-LICENSE.txt "$prefix/share/doc/folio/INTER-LICENSE.txt"
install -Dm644 assets/INTER-APACHE-2.0.txt "$prefix/share/doc/folio/INTER-APACHE-2.0.txt"
install -Dm644 README.md "$prefix/share/doc/folio/README.md"
install -Dm644 SECURITY.md "$prefix/share/doc/folio/SECURITY.md"
install -Dm644 CHANGELOG.md "$prefix/share/doc/folio/CHANGELOG.md"
for document in docs/*; do
    if [ -f "$document" ]; then
        install -Dm644 "$document" "$prefix/share/doc/folio/$document"
    fi
done
# Desktop launchers do not necessarily inherit ~/.local/bin in PATH.
desktop="$prefix/share/applications/dev.folio.Notes.desktop"
python3 - "$desktop" "$prefix/bin/folio" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
binary = sys.argv[2].replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$').replace('%', '%%')
path.write_text(path.read_text().replace('Exec=folio\n', f'Exec="{binary}"\n'))
PY
printf 'Installed Folio at %s/bin/folio\n' "$prefix"
