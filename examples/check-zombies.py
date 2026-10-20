#!/usr/bin/env python3
"""Example protocol-v1 check. Uses a numeric UID and never signals processes."""
import json
import os
import subprocess

uid = os.environ["URES_TARGET_UID"]
result = subprocess.run(["ps", "-u", uid, "-o", "stat="], capture_output=True,
                        text=True, timeout=1, check=False)
if result.returncode not in (0, 1):
    raise RuntimeError(result.stderr.strip())
count = sum(line.strip().startswith("Z") for line in result.stdout.splitlines())
print(json.dumps({"schema_version": 1, "status": "warn" if count else "ok",
                  "summary": f"{count} zombie processes",
                  "metrics": [{"id": "zombies", "value": count, "unit": "count"}]}))
