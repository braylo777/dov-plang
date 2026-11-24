#!/usr/bin/env python3
"""Keep the numbers in README.md equal to results/metrics.json.

The README holds a generated block between `<!-- numbers:start -->` and `<!-- numbers:end -->`
(badges + table). `--write` regenerates it; `--check` fails if it differs (CI runs this).
"""
import json, os, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
START, END = "<!-- numbers:start -->", "<!-- numbers:end -->"


def render(m):
    b = m.get("bench", {})
    k = b.get("kernels", {})
    c = b.get("compile", {})
    lines = [
        START,
        f"![tests](https://img.shields.io/badge/tests-{m['tests']}%20passing-brightgreen) "
        f"![examples](https://img.shields.io/badge/examples-{m['examples']}-blue) "
        f"![deps](https://img.shields.io/badge/dependencies-{m['deps']}-blue) "
        f"![version](https://img.shields.io/badge/version-{m['version']}-informational)",
        "",
        "| Metric | Value |",
        "|---|---|",
        f"| Tests passing (`cargo test`) | {m['tests']} |",
        f"| Golden cases (run / fail / fmt) | {m['golden_run']} / {m['golden_fail']} / {m['golden_fmt']} |",
        f"| Examples (`make examples`) | {m['examples']} |",
        f"| Crate dependencies | {m['deps']} |",
        f"| Rust lines (non-blank, non-comment) | {m['loc_rust']} |",
    ]
    if c:
        lines.append(f"| Compile time, `dovc emit-c` | {c['ms_per_1k_lines']} ms per 1,000 lines ({c['lines']} lines) |")
    for name, r in k.items():
        lines.append(f"| Kernel `{name}`: .dov / hand C | {r['dov_ms']} ms / {r['c_ms']} ms ({r['ratio']}x) |")
    lines += [
        "",
        f"Measured {m['generated_at']} at `{m['git_sha']}` on {m['machine']}"
        + (f" with {b['cc']}" if b.get("cc") else "")
        + ". Source: [`results/metrics.json`](results/metrics.json).",
        END,
    ]
    return "\n".join(lines)


def main():
    m = json.load(open(os.path.join(ROOT, "results", "metrics.json")))
    path = os.path.join(ROOT, "README.md")
    text = open(path).read()
    if START not in text or END not in text:
        sys.exit("README.md has no numbers block")
    a, rest = text.split(START, 1)
    _, b = rest.split(END, 1)
    new = a + render(m) + b
    if "--check" in sys.argv:
        if new != text:
            sys.exit("README numbers differ from results/metrics.json; run `python3 scripts/readme_numbers.py --write`")
        print("README numbers match results/metrics.json")
    else:
        open(path, "w").write(new)
        print("README numbers updated")


if __name__ == "__main__":
    main()
