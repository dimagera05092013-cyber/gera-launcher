#!/usr/bin/env bash
# Выпуск новой версии: поднимает номер, коммитит, ставит тег и отправляет в GitHub.
# GitHub Actions соберёт Windows и Linux и опубликует релиз.
set -euo pipefail

cd "$(dirname "$0")/.."

version="${1#v}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "Использование: scripts/release.sh 0.9.1" >&2
  exit 1
fi

if git rev-parse "v$version" >/dev/null 2>&1; then
  echo "Тег v$version уже существует." >&2
  exit 1
fi

python3 - "$version" <<'PY'
import json
import sys
from pathlib import Path

version = sys.argv[1]

for name in ("package.json", "src-tauri/tauri.conf.json"):
    path = Path(name)
    data = json.loads(path.read_text())
    data["version"] = version
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2) + "\n")

path = Path("src-tauri/Cargo.toml")
lines = path.read_text().splitlines()
for index, line in enumerate(lines):
    if line.startswith("version = "):
        lines[index] = f'version = "{version}"'
        break
path.write_text("\n".join(lines) + "\n")
PY

npm run check >/dev/null

git add -A
git commit -m "Release v$version" || true
git tag "v$version"
git push origin main
git push origin "v$version"

echo
echo "Тег v$version отправлен. GitHub Actions соберёт релиз:"
echo "  gh run watch"
echo "  gh release view v$version"
