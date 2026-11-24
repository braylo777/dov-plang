#!/usr/bin/env python3
"""Measure the repository and write results/metrics.json.

Runs `cargo test` (counts passing tests), counts examples, golden cases, dependencies and
Rust lines, and includes results/bench.json (run `make bench` first).
Usage: python3 scripts/metrics.py [--skip-tests]   (DOVC/CARGO_TARGET_DIR are respected)
"""
import datetime, json, os, re, subprocess, sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "bench"))


def sh(*cmd):
    return subprocess.run(cmd, cwd=ROOT, capture_output=True, text=True)


def count(dir_, ext):
    d = os.path.join(ROOT, dir_)
    return sum(1 for n in os.listdir(d) if n.endswith(ext) and not n.startswith("._"))


def loc_rust():
    n = 0
    src = os.path.join(ROOT, "crates", "dovc", "src")
    for f in sorted(os.listdir(src)):
        if f.endswith(".rs") and not f.startswith("._"):
            for line in open(os.path.join(src, f)):
                t = line.strip()
                if t and not t.startswith("//"):
                    n += 1
    return n


def deps():
    text = open(os.path.join(ROOT, "crates", "dovc", "Cargo.toml")).read()
    total = 0
    for section in ("dependencies", "dev-dependencies", "build-dependencies"):
        m = re.search(r"^\[" + section + r"\]\n(.*?)(?=^\[|\Z)", text, re.S | re.M)
        if m:
            total += sum(1 for l in m.group(1).splitlines() if l.strip() and not l.startswith("#"))
    return total


def tests():
    out = sh("cargo", "test", "--locked")
    text = out.stdout + out.stderr
    passed = sum(int(x) for x in re.findall(r"test result: \w+\. (\d+) passed", text))
    failed = sum(int(x) for x in re.findall(r"(\d+) failed", text))
    return passed, failed


def main():
    from run import machine

    passed, failed = (0, 0) if "--skip-tests" in sys.argv else tests()
    old = {}
    path = os.path.join(ROOT, "results", "metrics.json")
    if "--skip-tests" in sys.argv and os.path.exists(path):
        old = json.load(open(path))
        passed, failed = old.get("tests", 0), old.get("tests_failed", 0)
    bench_path = os.path.join(ROOT, "results", "bench.json")
    bench = json.load(open(bench_path)) if os.path.exists(bench_path) else {}
    version = re.search(r'^version = "([^"]+)"', open(os.path.join(ROOT, "Cargo.toml")).read(), re.M).group(1)
    m = {
        "version": version,
        "tests": passed,
        "tests_failed": failed,
        "examples": count("examples", ".dov"),
        "golden_run": count("tests/golden/run", ".dov"),
        "golden_fail": count("tests/golden/fail", ".dov"),
        "golden_fmt": count("tests/golden/fmt", ".dov"),
        "deps": deps(),
        "loc_rust": loc_rust(),
        "loc_rust_rule": "non-blank, non-comment lines in crates/dovc/src/*.rs",
        "bench": bench,
        "generated_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "git_sha": sh("git", "rev-parse", "--short", "HEAD").stdout.strip(),
        "machine": machine(),
    }
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        json.dump(m, f, indent=2)
        f.write("\n")
    print(json.dumps({k: v for k, v in m.items() if k != "bench"}, indent=2))
    if failed:
        sys.exit(f"{failed} test(s) failed")


if __name__ == "__main__":
    main()
