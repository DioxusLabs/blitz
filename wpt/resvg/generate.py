#!/usr/bin/env python3
"""Wrap resvg's test suite as WPT reftests that Blitz's WPT runner can run.

usage: generate.py <resvg checkout> <out dir>

resvg's suite (crates/resvg/tests) is one SVG per feature with a reference PNG
rendered by resvg. This writes, under <out dir>/resvg:

  tests/, resources/, fonts/  copied from resvg
  fonts.conf                  fontconfig file loading only resvg's fonts, with
                              the same generic families as resvg's test harness
  img/<path>.html             <img src=test.svg> vs resvg's PNG: the usvg backend
  inline/<path>.html          the SVG inlined vs resvg's PNG: whichever inline
                              backend the runner uses (BLITZ_NATIVE_SVG=1 for native)
  parity/<path>.html          the SVG inlined vs <img src=test.svg>: native vs usvg
                              with the same rasterizer (run with BLITZ_NATIVE_SVG=1)

Run with WPT_DIR=<out dir> and FONTCONFIG_FILE=<out dir>/resvg/fonts.conf
(`just wpt-resvg` does all of this).
"""

import html
import os
import re
import shutil
import struct
import sys

# `<meta name="blitz-fuzzy" content="<threshold>;<pixels>">` (a Blitz runner
# extension): at most <pixels> pixels may differ by more than <threshold>.
# resvg's references come from tiny-skia, not vello_cpu, so anti-aliased edges
# and gradients differ slightly almost everywhere: allow 0.5% of a 300x300
# render to differ by more than 64/255.
PNG_FUZZY = "64;450"
# Native vs usvg runs through the same rasterizer.
PARITY_FUZZY = "16;90"

FONT_FAMILIES = {
    # Matches GLOBAL_FONTDB in resvg's crates/resvg/tests/integration/main.rs
    "serif": "Noto Serif",
    "sans-serif": "Noto Sans",
    "cursive": "Yellowtail",
    "fantasy": "Sedgwick Ave Display",
    "monospace": "Noto Mono",
}

PROLOG_RE = re.compile(r"^\s*(<\?xml[^>]*\?>\s*)?(<!--.*?-->\s*)*(<!DOCTYPE[^>\[]*(\[.*?\])?\s*>)?", re.S)


def decode_xml(data):
    """Decodes an SVG with the encoding its XML declaration names, as usvg does."""
    encoding = re.match(rb'\s*<\?xml[^>]*encoding="([^"]+)"', data)
    return data.decode(encoding.group(1).decode() if encoding else "utf-8")


def png_size(path):
    with open(path, "rb") as f:
        return struct.unpack(">II", f.read(24)[16:24])


def page(title, fuzzy, ref, body, width, height):
    head = "<!DOCTYPE html>\n<meta charset=utf-8>\n<title>%s</title>\n" % html.escape(title)
    if ref:
        head += '<link rel="match" href="%s">\n' % ref
    if fuzzy:
        head += '<meta name="blitz-fuzzy" content="%s">\n' % fuzzy
    style = "<style>body { margin: 0 } body > img, body > svg { display: block; width: %dpx; height: %dpx }</style>\n" % (width, height)
    return head + style + "<body>\n" + body + "\n"


def write_fonts_conf(dst):
    fonts_dir = os.path.abspath(os.path.join(dst, "fonts"))
    aliases = "".join(
        "  <alias><family>%s</family><prefer><family>%s</family></prefer></alias>\n" % (generic, family)
        for generic, family in FONT_FAMILIES.items()
    )
    with open(os.path.join(dst, "fonts.conf"), "w") as f:
        f.write(
            '<?xml version="1.0"?>\n<!DOCTYPE fontconfig SYSTEM "fonts.dtd">\n<fontconfig>\n'
            "  <dir>%s</dir>\n  <cachedir>%s</cachedir>\n%s</fontconfig>\n"
            % (html.escape(fonts_dir), html.escape(fonts_dir + "-cache"), aliases)
        )


def main():
    if len(sys.argv) != 3:
        sys.exit(__doc__)
    src = os.path.join(sys.argv[1], "crates/resvg/tests")
    dst = os.path.join(sys.argv[2], "resvg")
    shutil.rmtree(dst, ignore_errors=True)
    for name in ("tests", "resources", "fonts"):
        shutil.copytree(os.path.join(src, name), os.path.join(dst, name))
    write_fonts_conf(dst)

    count = 0
    tests_dir = os.path.join(dst, "tests")
    for root, _, files in os.walk(tests_dir):
        rel_dir = os.path.relpath(root, tests_dir)
        # The runner treats `#` in a test path as a URL fragment
        for f in [f for f in files if "#" in f]:
            os.rename(os.path.join(root, f), os.path.join(root, f.replace("#", "_")))
        files = [f.replace("#", "_") for f in files]
        for f in sorted(files):
            name, ext = os.path.splitext(f)
            png = os.path.join(root, name + ".png")
            if ext != ".svg" or not os.path.exists(png):
                continue
            width, height = png_size(png)
            with open(os.path.join(root, f), "rb") as svg_file:
                svg_source = PROLOG_RE.sub("", decode_xml(svg_file.read()), count=1)
            # Pages sit at the same depth as the SVG, so relative hrefs inside
            # an inlined SVG (../../../resources/...) resolve the same way.
            up = "../" * (rel_dir.count("/") + 2)
            svg_url = "%stests/%s/%s" % (up, rel_dir, f)
            png_url = "%stests/%s/%s" % (up, rel_dir, name + ".png")
            title = "%s/%s" % (rel_dir, name)
            img_svg = '<img src="%s">' % svg_url
            img_png = '<img src="%s">' % png_url

            pages = {
                "img": [(name, PNG_FUZZY, img_svg), (name + "-ref", None, img_png)],
                "inline": [(name, PNG_FUZZY, svg_source), (name + "-ref", None, img_png)],
                "parity": [(name, PARITY_FUZZY, svg_source), (name + "-ref", None, img_svg)],
            }
            for mode, mode_pages in pages.items():
                out = os.path.join(dst, mode, rel_dir)
                os.makedirs(out, exist_ok=True)
                for page_name, fuzzy, body in mode_pages:
                    ref = name + "-ref.html" if fuzzy else None
                    with open(os.path.join(out, page_name + ".html"), "w", encoding="utf-8") as p:
                        p.write(page(title, fuzzy, ref, body, width, height))
            count += 1
    print("Generated %d tests in %s" % (count, dst))


if __name__ == "__main__":
    main()
