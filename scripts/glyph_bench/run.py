#!/usr/bin/env python3
"""Local glyph-caching benchmark runner for Blitz (GPU + CPU, cold / few-miss / warm).

Compares Blitz `main` (`glyph_cache=off` and `glyph_cache=on`) against the pinned
`anyrender` glyph-stack head (and optionally extra `--anyrender-rev` commits or a
local `--anyrender-path` checkout).

Examples:
    # Full 13-workload GPU + CPU matrix (main_off, main_on, stack head)
    python3 scripts/glyph_bench/run.py

    # Quick 6-workload GPU-only sanity check (~30s after build)
    python3 scripts/glyph_bench/run.py --backends gpu --workloads quick

    # Compare main against a local anyrender checkout
    python3 scripts/glyph_bench/run.py --anyrender-path ../anyrender

    # Compare main against multiple anyrender commits
    python3 scripts/glyph_bench/run.py --anyrender-rev 29f36f0 --anyrender-rev 8f8b83e
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import platform
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
FIXTURES_DIR = SCRIPT_DIR / "fixtures"
DEFAULT_REPO = SCRIPT_DIR.parent.parent

ANYRENDER_CRATES = [
    "anyrender",
    "anyrender_serialize",
    "anyrender_skia",
    "anyrender_tiny_skia",
    "anyrender_vello",
    "anyrender_vello_cpu",
    "anyrender_vello_gpu",
    "anyrender_svg",
    "wgpu_context",
    "image_filters_types",
    "pixels_window_renderer",
    "softbuffer_window_renderer",
]

STAT_RE = re.compile(
    r"^(paint_scene|rasterize  |total      ): "
    r"min (\d+)us / median (\d+)us / mean (\d+)us / p95 (\d+)us / max (\d+)us$"
)
COLD_RE = re.compile(
    r"^cold_frame : encode (\d+)us / rasterize (\d+)us / total (\d+)us$"
)
ADAPTER_RE = re.compile(r"^adapter\s*:\s*(.+)$")


def all_workloads(fixtures_dir: Path) -> list[tuple[str, str, str, str]]:
    """Return list of (name, url, scroll_arg, category)."""
    def fx(filename: str) -> str:
        return (fixtures_dir / filename).resolve().as_uri()

    return [
        ("synth_dense_prose",        fx("synth_dense_prose.html"),        "0",          "static"),
        ("synth_multisize",          fx("synth_multisize.html"),          "0",          "static"),
        ("synth_code_table",         fx("synth_code_table.html"),         "0",          "static"),
        ("synth_emoji_multilingual", fx("synth_emoji_multilingual.html"), "0",          "static"),
        ("synth_atlas_stress",       fx("synth_atlas_stress.html"),       "0",          "static"),
        ("synth_few_misses_static",  fx("synth_warm_few_misses.html"),    "0",          "static"),
        ("synth_few_misses_scroll",  fx("synth_warm_few_misses.html"),    "scroll:40",  "scroll"),
        ("wiki_rust_top",            "https://en.wikipedia.org/wiki/Rust_(programming_language)", "0",          "static"),
        ("wiki_rust_body",           "https://en.wikipedia.org/wiki/Rust_(programming_language)", "1200",       "static"),
        ("wiki_rust_scroll",         "https://en.wikipedia.org/wiki/Rust_(programming_language)", "scroll:40",  "scroll"),
        ("wiki_us_body",             "https://en.wikipedia.org/wiki/United_States",               "1200",       "static"),
        ("wiki_us_scroll",           "https://en.wikipedia.org/wiki/United_States",               "scroll:120", "scroll"),
        ("whatwg_parsing",           "https://html.spec.whatwg.org/multipage/parsing.html",       "1200",       "static"),
    ]


QUICK_SET = {
    "synth_dense_prose",
    "synth_atlas_stress",
    "synth_few_misses_static",
    "synth_few_misses_scroll",
    "wiki_rust_body",
    "wiki_rust_scroll",
}


def run_cmd(cmd: list[str], cwd: Path | None = None, env: dict[str, str] | None = None) -> str:
    proc = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(
            f"Command failed ({proc.returncode}): {' '.join(cmd)}\n"
            f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )
    return proc.stdout.strip()


def git_rev_parse(repo: Path, ref: str) -> str:
    return run_cmd(["git", "rev-parse", "--short=9", ref], cwd=repo)


def extract_pinned_anyrender_rev(repo: Path, ref: str) -> str | None:
    try:
        text = run_cmd(["git", "show", f"{ref}:Cargo.toml"], cwd=repo)
    except RuntimeError:
        return None
    m = re.search(
        r'anyrender\s*=\s*\{\s*git\s*=\s*"[^"]+"\s*,\s*rev\s*=\s*"([0-9a-f]+)"',
        text,
    )
    return m.group(1)[:7] if m else None


def patch_cargo_toml_anyrender(cargo_toml: Path, *, rev: str | None = None, path: Path | None = None) -> None:
    text = cargo_toml.read_text()
    if rev is not None:
        lines = [
            f'{crate} = {{ git = "https://github.com/DioxusLabs/anyrender", rev = "{rev}" }}'
            for crate in ANYRENDER_CRATES
        ]
    elif path is not None:
        abs_path = path.resolve()
        lines = [
            f'{crate} = {{ path = "{(abs_path / "crates" / crate).as_posix()}" }}'
            for crate in ANYRENDER_CRATES
        ]
    else:
        return

    block = "[patch.crates-io]\n" + "\n".join(lines) + "\n"
    pattern = re.compile(
        r"\[patch\.crates-io\]\n(?:[a-z0-9_]+ = \{[^\n]+\}\n)+",
        re.MULTILINE,
    )
    if pattern.search(text):
        new_text = pattern.sub(block, text, count=1)
    else:
        new_text = text.rstrip() + "\n\n" + block
    cargo_toml.write_text(new_text)


def build_variant_binary(
    repo: Path,
    git_ref: str,
    dest_bin: Path,
    target_dir: Path,
    *,
    anyrender_rev: str | None = None,
    anyrender_path: Path | None = None,
) -> None:
    wt_root = Path(tempfile.mkdtemp(prefix="blitz_glyph_wt_"))
    wt_dir = wt_root / "wt"
    try:
        print(f"[build] creating worktree for {git_ref} -> {dest_bin.name}", flush=True)
        run_cmd(["git", "worktree", "add", "--detach", str(wt_dir), git_ref], cwd=repo)
        if anyrender_rev is not None or anyrender_path is not None:
            patch_cargo_toml_anyrender(
                wt_dir / "Cargo.toml",
                rev=anyrender_rev,
                path=anyrender_path,
            )
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = str(target_dir.resolve())
        cmd = ["cargo", "build", "--release", "--example", "paint_bench"]
        print(f"[build] running: {' '.join(cmd)}", flush=True)
        proc = subprocess.run(cmd, cwd=wt_dir, env=env)
        if proc.returncode != 0:
            raise RuntimeError(f"cargo build failed for {dest_bin.name} (ref {git_ref})")
        exe_suffix = ".exe" if os.name == "nt" else ""
        built = target_dir / "release" / "examples" / f"paint_bench{exe_suffix}"
        dest_bin.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(built, dest_bin)
        if os.name != "nt":
            dest_bin.chmod(0o755)
        print(f"[build] ready: {dest_bin}", flush=True)
    finally:
        subprocess.run(
            ["git", "worktree", "remove", "--force", str(wt_dir)],
            cwd=repo,
            capture_output=True,
        )
        shutil.rmtree(wt_root, ignore_errors=True)
        subprocess.run(["git", "worktree", "prune"], cwd=repo, capture_output=True)


def parse_output(stdout: str) -> dict:
    parsed: dict = {}
    for line in stdout.splitlines():
        m = ADAPTER_RE.match(line)
        if m:
            parsed["adapter"] = m.group(1).strip()
        m = COLD_RE.match(line)
        if m:
            parsed["cold"] = {
                "encode_us": int(m.group(1)),
                "raster_us": int(m.group(2)),
                "total_us": int(m.group(3)),
            }
        m = STAT_RE.match(line)
        if m:
            key = m.group(1).strip()
            parsed[key] = {
                "min_us": int(m.group(2)),
                "median_us": int(m.group(3)),
                "mean_us": int(m.group(4)),
                "p95_us": int(m.group(5)),
                "max_us": int(m.group(6)),
            }
    return parsed


def run_benchmark(
    binary: Path,
    url: str,
    width: int,
    height: int,
    scale: str,
    iters: int,
    backend: str,
    cache_arg: str,
    scroll: str,
    timeout: int,
    extra_env: dict[str, str] | None = None,
) -> tuple[int, str, str]:
    cmd = [
        str(binary),
        url,
        str(width),
        str(height),
        str(scale),
        str(iters),
        backend,
        cache_arg,
        scroll,
    ]
    env = os.environ.copy()
    if extra_env:
        env.update(extra_env)
    proc = subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, env=env)
    return proc.returncode, proc.stdout, proc.stderr


def fmt_ms(us: float | int | None) -> str:
    if us is None:
        return "ERR"
    return f"{us / 1000.0:.2f}"


def fmt_ratio(num: float | int | None, den: float | int | None) -> str:
    if not num or not den:
        return "—"
    return f"{num / den:.2f}x"


def summarize_cold(entry: dict) -> str:
    if entry.get("error"):
        return "PANIC"
    c = entry.get("cold_median")
    if not c:
        return "ERR"
    return f"{fmt_ms(c['encode_us'])} / {fmt_ms(c['total_us'])}"


def summarize_warm(entry: dict, include_p95: bool = False) -> str:
    if entry.get("error"):
        return "PANIC"
    enc = entry.get("paint_scene")
    tot = entry.get("total")
    if not enc or not tot:
        return "ERR"
    if include_p95:
        return f"{fmt_ms(enc['median_us'])} ({fmt_ms(enc['p95_us'])}) / {fmt_ms(tot['median_us'])}"
    return f"{fmt_ms(enc['median_us'])} / {fmt_ms(tot['median_us'])}"


def render_markdown(
    results: dict,
    workloads: list[tuple[str, str, str, str]],
    variants: list[tuple[str, str, str]],
    backends: list[str],
    meta: dict,
) -> str:
    lines: list[str] = []
    lines.append("# Blitz Glyph Caching Local Benchmark Summary")
    lines.append("")
    lines.append(f"- **Date:** {meta['timestamp']}")
    lines.append(
        f"- **Machine:** {meta['platform']} ({meta['machine']}, {meta['cpus']} CPUs), Rust `{meta['rustc']}`"
    )
    if meta.get("adapter"):
        lines.append(f"- **GPU adapter:** `{meta['adapter']}`")
    lines.append(
        f"- **Viewport:** `{meta['viewport']}`, cold runs: `{meta['cold_runs']}`, warm iters: `{meta['iters']}`"
    )
    lines.append(
        "- **Variants:** "
        + ", ".join(f"`{v}` ({desc})" for v, desc in meta["variant_descriptions"].items())
    )
    lines.append("")

    variant_names = [v[0] for v in variants]
    compare_target = variant_names[-1]
    has_main_on = "main_on" in variant_names and compare_target != "main_on"

    # 1. Cold frames
    for backend in backends:
        lines.append(f"## Cold frames — {backend.upper()} (frame 0, {meta['cold_runs']}-run median ms, `enc / tot`)")
        lines.append("")
        hdr = ["Workload"] + [f"`{v}`" for v in variant_names]
        if has_main_on:
            hdr.append(f"`{compare_target}/main_on` enc")
        lines.append("| " + " | ".join(hdr) + " |")
        lines.append("|---|" + "---:|" * (len(hdr) - 1))
        for wname, _, _, _ in workloads:
            row = [f"`{wname}`"]
            for v in variant_names:
                row.append(summarize_cold(results[backend][wname][v]))
            if has_main_on:
                c_t = results[backend][wname][compare_target].get("cold_median", {}).get("encode_us")
                c_m = results[backend][wname]["main_on"].get("cold_median", {}).get("encode_us")
                row.append(fmt_ratio(c_t, c_m))
            lines.append("| " + " | ".join(row) + " |")
        lines.append("")

    # 2. Warm — few misses (scroll)
    scroll_workloads = [w for w in workloads if w[3] == "scroll"]
    if scroll_workloads:
        lines.append(f"## Warm — handful of cache misses (scroll, {meta['iters']} iters, `enc (p95) / tot` ms)")
        lines.append("")
        hdr = ["Workload", "Backend"] + [f"`{v}`" for v in variant_names]
        if has_main_on:
            hdr.append(f"`{compare_target}/main_on` enc")
        lines.append("| " + " | ".join(hdr) + " |")
        lines.append("|---|---|" + "---:|" * (len(hdr) - 2))
        for backend in backends:
            for wname, _, _, _ in scroll_workloads:
                row = [f"`{wname}`", backend.upper()]
                for v in variant_names:
                    row.append(summarize_warm(results[backend][wname][v], include_p95=True))
                if has_main_on:
                    e_t = results[backend][wname][compare_target].get("paint_scene", {}).get("median_us")
                    e_m = results[backend][wname]["main_on"].get("paint_scene", {}).get("median_us")
                    row.append(fmt_ratio(e_t, e_m))
                lines.append("| " + " | ".join(row) + " |")
        lines.append("")

    # 3. Warm — 0 misses (static)
    static_workloads = [w for w in workloads if w[3] == "static"]
    if static_workloads:
        for backend in backends:
            lines.append(f"## Warm — 0 cache misses — {backend.upper()} ({meta['iters']} iters, `enc / tot` ms)")
            lines.append("")
            hdr = ["Workload"] + [f"`{v}`" for v in variant_names]
            if has_main_on:
                hdr.append(f"`{compare_target}/main_on` enc")
            lines.append("| " + " | ".join(hdr) + " |")
            lines.append("|---|" + "---:|" * (len(hdr) - 1))
            for wname, _, _, _ in static_workloads:
                row = [f"`{wname}`"]
                for v in variant_names:
                    row.append(summarize_warm(results[backend][wname][v], include_p95=False))
                if has_main_on:
                    e_t = results[backend][wname][compare_target].get("paint_scene", {}).get("median_us")
                    e_m = results[backend][wname]["main_on"].get("paint_scene", {}).get("median_us")
                    row.append(fmt_ratio(e_t, e_m))
                lines.append("| " + " | ".join(row) + " |")
            lines.append("")

    # 4. Fresh atlas GPU (optional)
    if "fresh_atlas_gpu" in results and results["fresh_atlas_gpu"]:
        lines.append(f"## Fresh-atlas GPU (`BENCH_FRESH_ATLAS=1`, `enc / ras / tot` median ms)")
        lines.append("")
        hdr = ["Workload"] + [f"`{v}`" for v in variant_names]
        lines.append("| " + " | ".join(hdr) + " |")
        lines.append("|---|" + "---:|" * (len(hdr) - 1))
        for wname, vmap in results["fresh_atlas_gpu"].items():
            row = [f"`{wname}`"]
            for v in variant_names:
                e = vmap.get(v, {})
                if e.get("error"):
                    row.append("PANIC")
                else:
                    enc = e.get("paint_scene", {}).get("median_us")
                    ras = e.get("rasterize", {}).get("median_us")
                    tot = e.get("total", {}).get("median_us")
                    row.append(f"{fmt_ms(enc)} / {fmt_ms(ras)} / {fmt_ms(tot)}")
            lines.append("| " + " | ".join(row) + " |")
        lines.append("")

    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Run local Blitz glyph caching benchmarks (main_off, main_on, and anyrender stack)."
    )
    parser.add_argument("--repo", type=Path, default=DEFAULT_REPO, help="Path to Blitz git repo")
    parser.add_argument("--main-ref", default="HEAD~1", help="Git ref for Blitz main paint_bench (default: HEAD~1)")
    parser.add_argument("--stack-ref", default="HEAD", help="Git ref for pinned anyrender stack paint_bench (default: HEAD)")
    parser.add_argument(
        "--anyrender-rev",
        action="append",
        default=[],
        help="Additional/replacement anyrender commit SHA(s) to benchmark (repeatable)",
    )
    parser.add_argument(
        "--anyrender-path",
        type=Path,
        default=None,
        help="Benchmark a local anyrender checkout instead of the pinned git rev",
    )
    parser.add_argument(
        "--backends",
        default="gpu,cpu",
        help="Comma-separated backends: gpu,cpu (default: gpu,cpu)",
    )
    parser.add_argument(
        "--workloads",
        default="all",
        help="all | synth | real | quick | comma-separated workload names (default: all)",
    )
    parser.add_argument("--iters", type=int, default=30, help="Warm iterations per run (default: 30)")
    parser.add_argument("--cold-runs", type=int, default=3, help="Runs for cold-frame median (default: 3)")
    parser.add_argument("--viewport", default="1366x768@2", help="Viewport WxH@scale (default: 1366x768@2)")
    parser.add_argument("--fresh-atlas", action="store_true", help="Also run GPU BENCH_FRESH_ATLAS=1 on wiki + stress")
    parser.add_argument("--skip-build", action="store_true", help="Reuse cached binaries in <out-dir>/bin")
    parser.add_argument(
        "--out-dir",
        type=Path,
        default=None,
        help="Output directory (default: <repo>/target/glyph_bench_out)",
    )
    parser.add_argument(
        "--target-dir",
        type=Path,
        default=None,
        help="Cargo target directory shared across builds (default: <repo>/target)",
    )
    parser.add_argument("--timeout", type=int, default=180, help="Per-run timeout in seconds (default: 180)")
    args = parser.parse_args()

    repo = args.repo.resolve()
    out_dir = (args.out_dir or (repo / "target" / "glyph_bench_out")).resolve()
    bin_dir = out_dir / "bin"
    target_dir = (args.target_dir or (repo / "target")).resolve()
    args.iters = max(1, args.iters)
    args.cold_runs = max(1, args.cold_runs)
    out_dir.mkdir(parents=True, exist_ok=True)
    bin_dir.mkdir(parents=True, exist_ok=True)

    m_vp = re.match(r"^(\d+)x(\d+)@([0-9.]+)$", args.viewport)
    if not m_vp:
        print(f"Invalid --viewport {args.viewport!r}; expected WxH@scale (e.g. 1366x768@2)", file=sys.stderr)
        return 2
    vp_w, vp_h, vp_scale = int(m_vp.group(1)), int(m_vp.group(2)), m_vp.group(3)

    backends = [b.strip().lower() for b in args.backends.split(",") if b.strip()]
    for b in backends:
        if b not in ("gpu", "cpu"):
            print(f"Unknown backend {b!r}; expected gpu and/or cpu", file=sys.stderr)
            return 2

    workloads_all = all_workloads(FIXTURES_DIR)
    if args.workloads == "all":
        workloads = workloads_all
    elif args.workloads == "synth":
        workloads = [w for w in workloads_all if w[0].startswith("synth_")]
    elif args.workloads == "real":
        workloads = [w for w in workloads_all if not w[0].startswith("synth_")]
    elif args.workloads == "quick":
        workloads = [w for w in workloads_all if w[0] in QUICK_SET]
    else:
        wanted = {x.strip() for x in args.workloads.split(",") if x.strip()}
        workloads = [w for w in workloads_all if w[0] in wanted]
        missing = wanted - {w[0] for w in workloads}
        if missing:
            known = ", ".join(w[0] for w in workloads_all)
            print(f"Unknown workloads: {', '.join(sorted(missing))}\nKnown: {known}", file=sys.stderr)
            return 2

    exe_suffix = ".exe" if os.name == "nt" else ""
    main_bin = bin_dir / f"paint_bench_main{exe_suffix}"
    main_sha = git_rev_parse(repo, args.main_ref)
    stack_sha = git_rev_parse(repo, args.stack_ref)
    pinned_rev = extract_pinned_anyrender_rev(repo, args.stack_ref) or "stack"

    # Build plan: (binary_path, git_ref, anyrender_rev, anyrender_path)
    build_jobs: list[tuple[Path, str, str | None, Path | None]] = [
        (main_bin, args.main_ref, None, None),
    ]
    # Variants: (name, binary_path, cache_arg)
    variants: list[tuple[str, Path, str]] = [
        ("main_off", main_bin, "off"),
        ("main_on", main_bin, "on"),
    ]
    variant_descriptions: dict[str, str] = {
        "main_off": f"Blitz {main_sha}, glyph_caching=false",
        "main_on": f"Blitz {main_sha}, glyph_caching=true",
    }

    if args.anyrender_path is not None:
        local_bin = bin_dir / f"paint_bench_local{exe_suffix}"
        build_jobs.append((local_bin, args.stack_ref, None, args.anyrender_path))
        variants.append(("local", local_bin, "on"))
        variant_descriptions["local"] = f"local anyrender @ {args.anyrender_path.resolve()}"
    elif args.anyrender_rev:
        for rev in args.anyrender_rev:
            short = rev[:7]
            rev_bin = bin_dir / f"paint_bench_{short}{exe_suffix}"
            build_jobs.append((rev_bin, args.stack_ref, rev, None))
            variants.append((short, rev_bin, "on"))
            variant_descriptions[short] = f"anyrender @ {rev}"
    else:
        stack_bin = bin_dir / f"paint_bench_{pinned_rev}{exe_suffix}"
        build_jobs.append((stack_bin, args.stack_ref, None, None))
        variants.append((pinned_rev, stack_bin, "on"))
        variant_descriptions[pinned_rev] = f"Blitz {stack_sha} pinned to anyrender @ {pinned_rev}"

    if not args.skip_build:
        for dest_bin, git_ref, ar_rev, ar_path in build_jobs:
            build_variant_binary(
                repo,
                git_ref,
                dest_bin,
                target_dir,
                anyrender_rev=ar_rev,
                anyrender_path=ar_path,
            )
    else:
        for dest_bin, _, _, _ in build_jobs:
            if not dest_bin.exists():
                print(f"--skip-build specified, but {dest_bin} does not exist", file=sys.stderr)
                return 2

    rustc_ver = run_cmd(["rustc", "--version"])
    adapter_seen: str | None = None
    results: dict = {}
    raw_lines: list[str] = []

    for backend in backends:
        results[backend] = {}
        print(f"\n=== BACKEND: {backend.upper()} ===", flush=True)
        for wname, url, scroll, _ in workloads:
            results[backend][wname] = {}
            for vname, binary, cache_arg in variants:
                cold_enc_list: list[int] = []
                cold_ras_list: list[int] = []
                cold_tot_list: list[int] = []
                last_parsed: dict | None = None
                err_msg: str | None = None

                for r in range(args.cold_runs):
                    run_iters = args.iters if r == 0 else 1
                    try:
                        rc, out, err = run_benchmark(
                            binary,
                            url,
                            vp_w,
                            vp_h,
                            vp_scale,
                            run_iters,
                            backend,
                            cache_arg,
                            scroll,
                            args.timeout,
                        )
                    except subprocess.TimeoutExpired:
                        err_msg = f"TIMEOUT ({args.timeout}s)"
                        break

                    raw_lines.append(f"--- {backend} {wname} {vname} run={r} rc={rc} ---\n{out}\n{err}\n")
                    if rc != 0:
                        err_msg = f"rc={rc}: {(err or out).strip()[:200]}"
                        break
                    parsed = parse_output(out)
                    if adapter_seen is None and "adapter" in parsed:
                        adapter_seen = parsed["adapter"]
                    if "cold" in parsed:
                        cold_enc_list.append(parsed["cold"]["encode_us"])
                        cold_ras_list.append(parsed["cold"]["raster_us"])
                        cold_tot_list.append(parsed["cold"]["total_us"])
                    if r == 0:
                        last_parsed = parsed

                if err_msg:
                    results[backend][wname][vname] = {"error": err_msg}
                    print(f"  {wname:26s} {vname:10s} ERROR: {err_msg}", flush=True)
                else:
                    entry = dict(last_parsed or {})
                    if cold_enc_list:
                        entry["cold_median"] = {
                            "encode_us": int(statistics.median(cold_enc_list)),
                            "raster_us": int(statistics.median(cold_ras_list)),
                            "total_us": int(statistics.median(cold_tot_list)),
                        }
                    results[backend][wname][vname] = entry
                    ce = entry.get("cold_median", {}).get("encode_us", 0) / 1000.0
                    we = entry.get("paint_scene", {}).get("median_us", 0) / 1000.0
                    wt = entry.get("total", {}).get("median_us", 0) / 1000.0
                    print(
                        f"  {wname:26s} {vname:10s} cold_enc={ce:6.2f}ms  warm_enc={we:5.2f}ms  warm_tot={wt:5.2f}ms",
                        flush=True,
                    )

    if args.fresh_atlas and "gpu" in backends:
        fa_targets = {"synth_atlas_stress", "wiki_rust_top", "wiki_rust_body", "wiki_us_body"}
        fa_workloads = [w for w in workloads if w[0] in fa_targets]
        if fa_workloads:
            results["fresh_atlas_gpu"] = {}
            print("\n=== FRESH-ATLAS GPU (BENCH_FRESH_ATLAS=1) ===", flush=True)
            for wname, url, scroll, _ in fa_workloads:
                results["fresh_atlas_gpu"][wname] = {}
                for vname, binary, cache_arg in variants:
                    try:
                        rc, out, err = run_benchmark(
                            binary,
                            url,
                            vp_w,
                            vp_h,
                            vp_scale,
                            15,
                            "gpu",
                            cache_arg,
                            scroll,
                            args.timeout,
                            extra_env={"BENCH_FRESH_ATLAS": "1"},
                        )
                    except subprocess.TimeoutExpired:
                        results["fresh_atlas_gpu"][wname][vname] = {"error": f"TIMEOUT ({args.timeout}s)"}
                        print(f"  {wname:26s} {vname:10s} TIMEOUT ({args.timeout}s)", flush=True)
                        continue
                    raw_lines.append(f"--- fresh_atlas gpu {wname} {vname} rc={rc} ---\n{out}\n{err}\n")
                    if rc != 0:
                        results["fresh_atlas_gpu"][wname][vname] = {"error": f"rc={rc}"}
                        print(f"  {wname:26s} {vname:10s} ERROR rc={rc}", flush=True)
                    else:
                        parsed = parse_output(out)
                        results["fresh_atlas_gpu"][wname][vname] = parsed
                        we = parsed.get("paint_scene", {}).get("median_us", 0) / 1000.0
                        wr = parsed.get("rasterize", {}).get("median_us", 0) / 1000.0
                        wt = parsed.get("total", {}).get("median_us", 0) / 1000.0
                        print(
                            f"  {wname:26s} {vname:10s} enc={we:6.2f}ms  ras={wr:6.2f}ms  tot={wt:6.2f}ms",
                            flush=True,
                        )

    meta = {
        "timestamp": dt.datetime.now(dt.timezone.utc).strftime("%Y-%m-%d %H:%M UTC"),
        "platform": f"{platform.system()} {platform.release()}",
        "machine": platform.machine(),
        "cpus": os.cpu_count(),
        "rustc": rustc_ver,
        "adapter": adapter_seen,
        "viewport": args.viewport,
        "iters": args.iters,
        "cold_runs": args.cold_runs,
        "variant_descriptions": variant_descriptions,
    }

    summary_md = render_markdown(
        results,
        workloads,
        [(v[0], str(v[1]), v[2]) for v in variants],
        backends,
        meta,
    )

    (out_dir / "results.json").write_text(json.dumps({"meta": meta, "results": results}, indent=2))
    (out_dir / "summary.md").write_text(summary_md)
    (out_dir / "raw.log").write_text("\n".join(raw_lines))

    print("\n" + summary_md, flush=True)
    print(f"Saved JSON:    {out_dir / 'results.json'}")
    print(f"Saved summary: {out_dir / 'summary.md'}")
    print(f"Saved raw log: {out_dir / 'raw.log'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
