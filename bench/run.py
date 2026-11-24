#!/usr/bin/env python3
"""Benchmarks for dovc. Writes results/bench.json.

1. Compile time: a generated 10,000-line program, `dovc emit-c` (front end + C generation),
   reported as milliseconds per 1,000 lines (median of 5 runs).
2. Kernels: each bench/kernels/NAME.dov vs a hand-written NAME.c, both built with `cc -O2`.
   Median wall time of 5 runs; outputs must match. .dov keeps its checked arithmetic and
   bounds checks, the C versions do not.
"""
import json, os, platform, statistics, subprocess, sys, tempfile, time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOVC = os.environ.get("DOVC", os.path.join(ROOT, "target", "release", "dovc"))
CC = os.environ.get("CC", "cc")
RUNS = 5
KERNELS = ["fib", "primes", "collatz", "slices"]


def machine():
    """OS, CPU architecture (from uname, not from the Python build) and CPU model."""
    arch = subprocess.run(["uname", "-m"], capture_output=True, text=True).stdout.strip()
    cpu = ""
    if platform.system() == "Darwin":
        cpu = subprocess.run(["sysctl", "-n", "machdep.cpu.brand_string"], capture_output=True, text=True).stdout.strip()
    elif os.path.exists("/proc/cpuinfo"):
        for line in open("/proc/cpuinfo"):
            if line.startswith("model name"):
                cpu = line.split(":", 1)[1].strip()
                break
    return f"{platform.system()} {arch} {cpu}".strip()


def timed(cmd, **kw):
    t = time.perf_counter()
    out = subprocess.run(cmd, check=True, capture_output=True, text=True, **kw)
    return time.perf_counter() - t, out.stdout


def synthetic(lines_target):
    parts = ["struct P { x: i64, y: i64 }\n\n"]
    n = 0
    i = 0
    while sum(p.count("\n") for p in parts) < lines_target:
        parts.append(
            f"fn f{i}(a: i64, b: i64) -> i64 {{\n"
            f"    var s = a;\n"
            f"    var k = 0;\n"
            f"    while k < b {{\n"
            f"        s = s + k * {i % 7 + 1};\n"
            f"        k = k + 1;\n"
            f"    }}\n"
            f"    let p = P {{ x: s, y: b }};\n"
            f"    if p.x > p.y {{\n"
            f"        return p.x - p.y;\n"
            f"    }}\n"
            f"    return p.y;\n"
            f"}}\n\n"
        )
        i += 1
    n = i
    parts.append("fn main() {\n    var t = 0;\n")
    for j in range(0, n, max(1, n // 50)):
        parts.append(f"    t = t + f{j}(1, 2);\n")
    parts.append("    print(t);\n}\n")
    return "".join(parts)


def compile_time(tmp):
    src = synthetic(10000)
    lines = src.count("\n")
    path = os.path.join(tmp, "big.dov")
    with open(path, "w") as f:
        f.write(src)
    times = [timed([DOVC, "emit-c", path])[0] for _ in range(RUNS)]
    ms = statistics.median(times) * 1000
    return {"lines": lines, "emit_c_ms": round(ms, 1), "ms_per_1k_lines": round(ms * 1000 / lines, 2)}


def kernel(tmp, name):
    dov_exe = os.path.join(tmp, name + "_dov")
    c_exe = os.path.join(tmp, name + "_c")
    subprocess.run([DOVC, "build", os.path.join(ROOT, "bench", "kernels", name + ".dov"), "-o", dov_exe], check=True)
    subprocess.run([CC, "-std=c11", "-O2", "-o", c_exe, os.path.join(ROOT, "bench", "kernels", name + ".c")], check=True)
    d = [timed([dov_exe]) for _ in range(RUNS)]
    c = [timed([c_exe]) for _ in range(RUNS)]
    if d[0][1] != c[0][1]:
        sys.exit(f"{name}: outputs differ: {d[0][1]!r} vs {c[0][1]!r}")
    dm = statistics.median(t for t, _ in d) * 1000
    cm = statistics.median(t for t, _ in c) * 1000
    return {"dov_ms": round(dm, 1), "c_ms": round(cm, 1), "ratio": round(dm / cm, 2), "output": d[0][1].strip()}


def main():
    with tempfile.TemporaryDirectory() as tmp:
        res = {
            "compile": compile_time(tmp),
            "kernels": {k: kernel(tmp, k) for k in KERNELS},
            "cc": subprocess.run([CC, "--version"], capture_output=True, text=True).stdout.splitlines()[0],
            "machine": machine(),
        }
    os.makedirs(os.path.join(ROOT, "results"), exist_ok=True)
    with open(os.path.join(ROOT, "results", "bench.json"), "w") as f:
        json.dump(res, f, indent=2)
        f.write("\n")
    print(json.dumps(res, indent=2))


if __name__ == "__main__":
    main()
