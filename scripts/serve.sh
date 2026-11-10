#!/usr/bin/env bash
# Serves the web app on http://localhost:5173 (wallet sign-in works on localhost).
cd "$(dirname "$0")/../web" || exit 1

# Pick an interpreter that actually runs. On Windows, `python`/`python3` on PATH is
# often a Microsoft Store stub that exits instead of starting, so probe it first.
for py in python3 python py /d/Python/python.exe /c/Python312/python.exe; do
  if command -v "$py" >/dev/null 2>&1 && "$py" --version >/dev/null 2>&1; then
    exec "$py" -m http.server 5173
  fi
done

if command -v npx >/dev/null 2>&1; then
  exec npx --yes http-server -p 5173 -c-1
fi

echo "Need a working python or npx on PATH to serve web/" >&2
exit 1
