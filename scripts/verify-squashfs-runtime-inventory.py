#!/usr/bin/env python3
"""Internal read-only inventory verifier used inside the SquashFS mount namespace."""
import importlib.util
import json
from pathlib import Path
import sys

builder_path = Path(__file__).with_name("build-wine-squashfs-image.py")
spec = importlib.util.spec_from_file_location("p034_builder", builder_path)
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)

root = Path(sys.argv[1])
evidence = json.loads(Path(sys.argv[2]).read_text(encoding="utf-8"))
actual = builder.inventory_tree(root)
if actual != evidence["logical_inventory"]:
    raise SystemExit("mounted filesystem inventory differs from image evidence")
print(json.dumps({"mounted_entry_count": len(actual),
                  "mounted_inventory_sha256": builder.digest_json(actual),
                  "image_sha256": evidence["image_sha256"]}, sort_keys=True))
