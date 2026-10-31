#!/usr/bin/env python3
"""Verify the exact generated shared core without requiring its sibling repo."""
import hashlib
import json
from pathlib import Path
root=Path(__file__).resolve().parents[1]/'agent/vendor/rustclipsync-core'
manifest=json.loads((root/'UPSTREAM.json').read_text())
expected=set(manifest['files'])|{'UPSTREAM.json'}
actual={str(p.relative_to(root)) for p in root.rglob('*') if p.is_file()}
if any(p.is_symlink() for p in root.rglob('*')) or expected!=actual:
    raise SystemExit('Shared core snapshot has unexpected or missing files.')
for name,digest in manifest['files'].items():
    if hashlib.sha256((root/name).read_bytes()).hexdigest()!=digest:
        raise SystemExit('Shared core snapshot differs: '+name)
print('Shared core snapshot hashes verified.')
