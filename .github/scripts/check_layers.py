"""Enforce the crate dependency rules of docs/01-architecture.md §2.1.

GEN-001  Layers 0-2: no third-party runtime (normal/build) dependencies.
GEN-002  A crate may depend only on lower layers (exception: aldis-spp -> aldis-time).
GEN-003  aldis-spp alone pulls in no frame-protocol crate.
GEN-004  Layer 3: third-party runtime dependencies only if optional.

Usage: cargo metadata --format-version 1 | python3 -I .github/scripts/check_layers.py
Temporary home until `xtask` exists (docs/milestones/m0-foundations.md WP-2).
"""
import json
import sys

LAYER = {
    "aldis-core": 0,
    "aldis-time": 1, "aldis-spp": 1, "aldis-epp": 1, "aldis-tm": 1,
    "aldis-tc": 1, "aldis-aos": 1, "aldis-uslp": 1,
    "aldis-link": 2, "aldis-cop1": 2,
    "aldis-framework": 3,
}
BOUNDARY = {"aldis", "aldis-ffi"}            # facade and C ABI: may use any layer
SAME_LAYER_ALLOWED = {("aldis-spp", "aldis-time")}
FRAME_CRATES = {"aldis-tm", "aldis-tc", "aldis-aos", "aldis-uslp", "aldis-link", "aldis-cop1"}

meta = json.load(sys.stdin)
members = {p["id"] for p in meta["packages"] if p["id"] in set(meta["workspace_members"])}
pkgs = {p["name"]: p for p in meta["packages"] if p["id"] in members}
errors = []

for name, pkg in sorted(pkgs.items()):
    if name not in LAYER and name not in BOUNDARY:
        errors.append(f"{name}: not assigned to a layer in check_layers.py (update docs/01-architecture.md §2 too)")
        continue
    for dep in pkg["dependencies"]:
        if dep["kind"] == "dev":
            continue
        d = dep["name"]
        if name in BOUNDARY:
            continue
        layer = LAYER[name]
        if d not in LAYER and d not in BOUNDARY:
            if layer <= 2:
                errors.append(f"GEN-001 {name} (L{layer}) has third-party dependency {d}")
            elif not dep["optional"]:
                errors.append(f"GEN-004 {name} (L{layer}) has non-optional third-party dependency {d}")
            continue
        if d in BOUNDARY:
            errors.append(f"GEN-002 {name} depends on boundary crate {d}")
            continue
        if LAYER[d] > layer or (LAYER[d] == layer and (name, d) not in SAME_LAYER_ALLOWED):
            errors.append(f"GEN-002 {name} (L{layer}) depends on {d} (L{LAYER[d]})")

# GEN-003: transitive normal-dependency closure of aldis-spp (all features).
closure, todo = set(), ["aldis-spp"]
while todo:
    cur = todo.pop()
    for dep in pkgs.get(cur, {}).get("dependencies", []):
        if dep["kind"] != "dev" and dep["name"] in pkgs and dep["name"] not in closure:
            closure.add(dep["name"])
            todo.append(dep["name"])
bad = sorted(closure & FRAME_CRATES)
if bad:
    errors.append(f"GEN-003 aldis-spp pulls in frame-protocol crates: {', '.join(bad)}")

for e in errors:
    print("error:", e)
print(f"checked {len(pkgs)} workspace crates: {'FAIL' if errors else 'ok'}")
sys.exit(1 if errors else 0)
