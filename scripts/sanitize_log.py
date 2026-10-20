"""Replace local filesystem/account identifiers before saving build logs."""
import os
import re
import sys
import check_privacy as privacy

LOCAL_TERMS = privacy.local_terms()

def redact(text):
    keys = ("URES_PROJECT", "URES_VENV", "URES_TOOLCHAIN", "URES_INSTALL_DIR",
            "CARGO_HOME", "CARGO_TARGET_DIR", "HOME")
    paths = {os.environ[k]: "${" + k + "}" for k in keys if os.environ.get(k)}
    if paths:
        text = re.sub("|".join(re.escape(k) for k in sorted(paths, key=len, reverse=True)),
                      lambda match: paths[match[0]], text)
    for key in ("USER", "LOGNAME"):
        if os.environ.get(key):
            text = re.sub(r"(?<![\w.-])" + re.escape(os.environ[key]) + r"(?![\w.-])",
                          lambda _: "${USER}", text)
    for value, category in LOCAL_TERMS:
        marker = {"local hostname": "HOSTNAME", "local UID": "UID",
                  "local account identifier": "USER", "local group identifier": "GROUP",
                  "local interface address": "IP"}.get(category, "PRIVATE_PATH")
        text = privacy.term_pattern(value, category).sub(lambda _: "${" + marker + "}", text)
    return text

if __name__ == "__main__":
    for line in sys.stdin:
        sys.stdout.write(redact(line))
        sys.stdout.flush()
