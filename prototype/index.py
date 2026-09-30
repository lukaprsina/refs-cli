#!/usr/bin/env python3
"""Throwaway prototype of SPEC.md §8 stage 2: index one repo within a byte budget.

Usage:
  index.py <checkout> --id ID --ref REF [--desc D] [--docs P...] [--code P...]
           [--examples P...] --budget BYTES [--full-out FILE] [--refs-dir .references]

Prints the inline block lines (header + truncated index) to stdout and a
floor/demand/used summary to stderr. With --full-out, writes the untruncated
two-hop index file. Lists files from `git ls-tree` at HEAD, not the worktree.
"""

import argparse
import re
import subprocess
import sys
from collections import defaultdict

DOC_EXTS = (".md", ".mdx", ".markdown", ".rst", ".adoc", ".asciidoc", ".txt")
KINDS = ("docs", "examples", "code")
MAX_SIZE = 1 << 20
CODE_DEPTH = 2


def natkey(s):
    return [(0, int(t), t) if t.isdigit() else (1, 0, t) for t in re.split(r"(\d+)", s)]


def ls_tree(repo, sha, paths):
    out = subprocess.run(
        ["git", "-C", repo, "ls-tree", "-r", "-l", sha, "--", *paths],
        check=True, capture_output=True, text=True,
    ).stdout
    for line in out.splitlines():
        meta, path = line.split("\t", 1)
        _, typ, _, size = meta.split()
        if typ != "blob":
            continue  # submodules
        yield path, int(size) if size != "-" else 0


def excluded(rel):
    return any(seg.startswith(".") or seg == "node_modules" for seg in rel.split("/"))


def collect(repo, sha, kind, base):
    """Return {dir: [entries]} for one configured path; dirs are repo-relative."""
    entries = defaultdict(set)
    for path, size in ls_tree(repo, sha, [base]):
        rel = path[len(base):].lstrip("/") if base else path
        if excluded(rel) or size > MAX_SIZE:
            continue
        d, _, name = path.rpartition("/")
        if kind == "docs" and not name.lower().endswith(DOC_EXTS):
            continue
        if kind == "code":
            parts = rel.split("/")[:-1][:CODE_DEPTH]
            for i in range(len(parts)):
                parent = "/".join([base, *parts[:i]]) if base else "/".join(parts[:i])
                entries[parent or "."].add(parts[i])
        else:
            entries[d or "."].add(name)
    return {d: sorted(e, key=natkey) for d, e in entries.items()}


def depth(d, base):
    rel = d[len(base):].strip("/") if base else ("" if d == "." else d)
    return 0 if not rel else rel.count("/") + 1


def build(repo, sha, paths):
    """List of (kind, dir, entries, bfs_key, hier_key)."""
    lines = []
    for ki, kind in enumerate(KINDS):
        for pi, base in enumerate(paths.get(kind, [])):
            base = base.strip("/")
            for d, ents in collect(repo, sha, kind, base).items():
                hier = (ki, pi, natkey(d))
                bfs = (depth(d, base), ki, pi, natkey(d))
                lines.append((kind, d, ents, bfs, hier))
    return lines


def fold(ents, n):
    """Collapse more than n same-extension siblings into one `*.ext(count)` token.
    Returns [(label, weight)], weight = how many entries the label stands for."""
    if not n:
        return [(e, 1) for e in ents]
    ext = lambda e: e.rpartition(".")[2].lower() if "." in e.lstrip(".") else None
    counts = defaultdict(int)
    for e in ents:
        counts[ext(e)] += 1
    out, done = [], set()
    for e in ents:
        x = ext(e)
        if x is None or counts[x] <= n:
            out.append((e, 1))
        elif x not in done:
            done.add(x)
            out.append((f"*.{x}({counts[x]})", counts[x]))
    return out


def render(kind, d, ents, extra=0):
    body = ",".join(ents + ([f"+{extra}"] if extra else []))
    return f"|{kind} {d}:{{{body}}}\n"


def blen(s):
    return len(s.encode())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("repo")
    ap.add_argument("--id", required=True)
    ap.add_argument("--ref", required=True)
    ap.add_argument("--desc", default="")
    ap.add_argument("--packages", nargs="*", default=[])
    ap.add_argument("--fold", type=int, default=0,
                    help="inline only: fold more than N same-extension siblings")
    ap.add_argument("--url", default="")
    ap.add_argument("--group", default="")
    for k in KINDS:
        ap.add_argument(f"--{k}", nargs="*", default=[])
    ap.add_argument("--budget", type=int, required=True)
    ap.add_argument("--full-out")
    ap.add_argument("--refs-dir", default=".references")
    a = ap.parse_args()

    sha = subprocess.run(["git", "-C", a.repo, "rev-parse", "HEAD"],
                         check=True, capture_output=True, text=True).stdout.strip()
    paths = {k: getattr(a, k) for k in KINDS}
    if not any(paths.values()):
        paths["docs"] = [""]
    full_path = f"{a.refs_dir}/{a.id}.md"

    at = f"@ {sha[:7]}" if re.fullmatch(r"[0-9a-f]{40}", a.ref) else f"@ {a.ref} {sha[:7]}"
    desc = f" {a.desc.rstrip('.')}." if a.desc else ""
    pkgs = f" Packages: {', '.join(a.packages)}." if a.packages else ""
    header = f"[{a.id} {at}]{desc}{pkgs} full index: {full_path}\n"

    lines = build(a.repo, sha, paths)
    hier = sorted(lines, key=lambda l: l[4])
    full_body = "".join(render(k, d, e) for k, d, e, _, _ in hier)
    total_entries = sum(len(e) for _, _, e, _, _ in lines)

    # From here on, entries are folded labels; the full index above stays unfolded.
    weights = {}
    for i, (k, d, e, bfs, h) in enumerate(lines):
        folded = fold(e, a.fold)
        weights[(k, d)] = [w for _, w in folded]
        lines[i] = (k, d, [lbl for lbl, _ in folded], bfs, h)
    hier = sorted(lines, key=lambda l: l[4])
    wsum = lambda k, d, n: sum(weights[(k, d)][:n])
    hid = lambda k, d, n: sum(weights[(k, d)]) - wsum(k, d, n)

    floor = blen(header)
    demand = floor + sum(blen(render(k, d, e)) for k, d, e, _, _ in lines)

    # Stage 2: breadth-first, whole lines while they fit, then one partial line, then stop.
    chosen = {}  # dir key -> number of entries shown
    used = floor
    shown = 0
    if demand <= a.budget:
        chosen = {(k, d): len(e) for k, d, e, _, _ in lines}
        used, shown = demand, total_entries
    else:
        # Reserve room for the summary line (upper bound on its digits).
        reserve = blen(f"|+{total_entries} entries in {len(lines)} dirs → {full_path}\n")
        room = a.budget - floor - reserve
        for kind, d, ents, _, _ in sorted(lines, key=lambda l: l[3]):
            whole = blen(render(kind, d, ents))
            if whole <= room:
                chosen[(kind, d)] = len(ents)
                room -= whole
                shown += wsum(kind, d, len(ents))
                continue
            k = len(ents) - 1
            while k > 0 and blen(render(kind, d, ents[:k], hid(kind, d, k))) > room:
                k -= 1
            if k > 0:
                chosen[(kind, d)] = k
                shown += wsum(kind, d, k)
            break

    out = [header]
    for kind, d, ents, _, _ in hier:
        n = chosen.get((kind, d))
        if n:
            out.append(render(kind, d, ents[:n], hid(kind, d, n)))
    hidden = total_entries - shown
    if hidden:
        hidden_dirs = sum(1 for k, d, e, _, _ in lines if chosen.get((k, d), 0) < len(e))
        out.append(f"|+{hidden} entries in {hidden_dirs} dirs → {full_path}\n")
    inline = "".join(out)
    sys.stdout.write(inline)
    print(f"{a.id}: floor={floor} demand={demand} budget={a.budget} used={blen(inline)} "
          f"entries={shown}/{total_entries}", file=sys.stderr)

    if a.full_out:
        kinds = "\n".join(f"- {k}: {', '.join(p) or '(repo root)'}" for k, p in paths.items() if p)
        doc = (
            f"# {a.id}\n\n"
            f"Generated by `refs sync`. Do not edit.\n\n"
            f"- url: {a.url}\n- group: {a.group}\n- ref: {a.ref}\n- sha: {sha}\n{kinds}\n\n"
            f"Paths below are relative to `{a.refs_dir}/{a.id}/`.\n\n"
            f"```\n{full_body}```\n"
        )
        with open(a.full_out, "w") as f:
            f.write(doc)


if __name__ == "__main__":
    main()
