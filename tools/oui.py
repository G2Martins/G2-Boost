"""Regenerate crates/core/data/oui.tsv from the IEEE MA-L public listing.

    curl -A "Mozilla/5.0" -o D:/dev/oui/oui.csv https://standards-oui.ieee.org/oui/oui.csv
    python -I tools/oui.py D:/dev/oui/oui.csv crates/core/data/oui.tsv

Output: one `AABBCC<TAB>Short name` line per 24-bit prefix, sorted, names shortened (legal
suffixes dropped, 28 chars max) so the table stays small inside the app.
"""
import csv
import re
import sys

SUFFIX = re.compile(
    r"[,.]?\s+(co\.?,?\s*ltd\.?|co\.?|ltd\.?|inc\.?|corp(oration)?\.?|llc|l\.l\.c\.|gmbh|ag|s\.?a\.?|s\.?r\.?l\.?|"
    r"b\.?v\.?|oy|ab|pty|plc|limited|company|technolog(y|ies)|electronics?|communications?|international|"
    r"holdings?|group|industrial|systems?)\b\.?",
    re.I,
)


def short(name: str) -> str:
    n = " ".join(name.replace("\t", " ").split())
    for _ in range(4):
        m = SUFFIX.search(n)
        if not m or m.start() < 3:
            break
        n = n[: m.start()].rstrip(" ,.-")
    return n[:28].rstrip(" ,.-")


def main(src: str, dst: str) -> None:
    rows = {}
    with open(src, encoding="utf-8", newline="") as f:
        for r in csv.DictReader(f):
            if r.get("Registry") != "MA-L":
                continue
            p = (r.get("Assignment") or "").strip().upper()
            name = short(r.get("Organization Name") or "")
            if re.fullmatch(r"[0-9A-F]{6}", p) and name and name.lower() != "private":
                rows[p] = name
    with open(dst, "w", encoding="utf-8", newline="\n") as f:
        for p in sorted(rows):
            f.write(f"{p}\t{rows[p]}\n")
    print(len(rows), "prefixes")


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
