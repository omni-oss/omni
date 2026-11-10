#!/usr/bin/env python3
"""Validate tasks.yaml and regenerate the wave tables in SCHEDULE.md.

Usage: python3 schedule.py           # validate + rewrite the generated block
       python3 schedule.py --check   # validate only (exit 1 on errors/stale output)
"""
import re
import sys
from collections import defaultdict
from pathlib import Path

import yaml

HERE = Path(__file__).parent
SIZE = {"XS": 0.5, "S": 1.5, "M": 4, "L": 8, "XL": 15}
BEGIN, END = "<!-- BEGIN GENERATED -->", "<!-- END GENERATED -->"

data = yaml.safe_load((HERE / "tasks.yaml").read_text())
tasks, domains = data["tasks"], data["domains"]
errors = []

ALLOWED = {"title", "parent", "kind", "size", "deps", "touches", "owner"}
for tid, t in tasks.items():
    extra = set(t) - ALLOWED
    if extra:
        errors.append(f"{tid}: unexpected keys {sorted(extra)} (unquoted comma in a title?)")
    if "title" in t and not isinstance(t["title"], str):
        errors.append(f"{tid}: title must be a string")
    for d in t["deps"]:
        if d not in tasks:
            errors.append(f"{tid}: unknown dep {d}")
    for dom in t["touches"]:
        if dom not in domains:
            errors.append(f"{tid}: unknown domain {dom}")

wave, finish, order = {}, {}, []
visiting = set()


def visit(tid):
    if tid in wave:
        return
    if tid in visiting:
        errors.append(f"cycle through {tid}")
        wave[tid] = 0
        finish[tid] = 0
        return
    visiting.add(tid)
    deps = [d for d in tasks[tid]["deps"] if d in tasks]
    for d in deps:
        visit(d)
    visiting.discard(tid)
    wave[tid] = 1 + max((wave[d] for d in deps), default=-1)
    finish[tid] = SIZE[tasks[tid]["size"]] + max((finish[d] for d in deps), default=0)
    order.append(tid)


for tid in tasks:
    visit(tid)

if errors:
    print("\n".join(errors))
    sys.exit(1)

# critical path
end = max(finish, key=finish.get)
path = [end]
while tasks[path[-1]]["deps"]:
    path.append(max(tasks[path[-1]]["deps"], key=lambda d: finish[d]))
path.reverse()

by_wave = defaultdict(list)
for tid in tasks:
    by_wave[wave[tid]].append(tid)

out = []
out.append(f"Tasks: {len(tasks)} · waves: {len(by_wave)} · "
           f"widest wave: {max(len(v) for v in by_wave.values())} · "
           f"critical path ≈ {finish[end]:.0f} working days\n")
out.append("**Critical path:** " + " → ".join(path) + "\n")
out.append("## Waves\n")
out.append("A task may start once every dependency is merged. All tasks in a wave are "
           "dependency-independent. `⚠` marks a conflict-domain overlap inside the wave "
           "(see the rules above).\n")
for w in sorted(by_wave):
    out.append(f"### Wave {w}\n")
    out.append("| Task | Title | Size | Deps | Domains |")
    out.append("| ---- | ----- | ---- | ---- | ------- |")
    seen = defaultdict(list)
    for tid in by_wave[w]:
        for dom in tasks[tid]["touches"]:
            if dom != "docs":
                seen[dom].append(tid)
    clash = {t for ts in seen.values() if len(ts) > 1 for t in ts}
    for tid in sorted(by_wave[w], key=lambda x: [int(p) if p.isdigit() else p for p in re.split(r"(\d+)", x)]):
        t = tasks[tid]
        flag = " ⚠" if tid in clash else ""
        out.append(f"| {tid}{flag} | {t['title']} | {t['size']} | "
                   f"{', '.join(t['deps']) or '-'} | {', '.join(t['touches']) or '-'} |")
    for dom, ts in seen.items():
        if len(ts) > 1:
            out.append(f"\n⚠ `{dom}` shared by {', '.join(ts)}: serialize or give to one worker.")
    out.append("")

gen = "\n".join(out)
sched = HERE / "SCHEDULE.md"
text = sched.read_text()
block = f"{BEGIN}\n{gen}\n{END}"
new = re.sub(re.escape(BEGIN) + r".*?" + re.escape(END), lambda m: block, text, flags=re.S)
if "--check" in sys.argv:
    if new != text:
        print("SCHEDULE.md is stale; run python3 schedule.py")
        sys.exit(1)
else:
    sched.write_text(new)
print(f"ok: {len(tasks)} tasks, {len(by_wave)} waves")
