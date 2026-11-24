#!/usr/bin/env python3
"""Render docs/ARCHITECTURE.md (with its SVG diagrams) to docs/ARCHITECTURE.pdf.

Needs Python-Markdown (`pip install markdown`) and Google Chrome (headless print-to-pdf).
Set CHROME to the Chrome binary if it is not in the default macOS location.
"""
import os, pathlib, subprocess, sys, tempfile

import markdown

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
CHROME = os.environ.get("CHROME", "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome")

CSS = """
body { font-family: -apple-system, Helvetica, Arial, sans-serif; font-size: 11pt; line-height: 1.45;
       color: #1f2933; max-width: 780px; margin: 0 auto; }
h1 { font-size: 22pt; border-bottom: 2px solid #2f6fb3; padding-bottom: 6px; }
h2 { font-size: 15pt; margin-top: 26px; border-bottom: 1px solid #cbd2d9; padding-bottom: 3px;
     page-break-after: avoid; }
code, pre { font-family: Menlo, Consolas, monospace; font-size: 8.6pt; }
pre { background: #f5f7fa; border: 1px solid #e4e7eb; padding: 8px; white-space: pre-wrap;
      page-break-inside: avoid; }
table { border-collapse: collapse; font-size: 9.5pt; }
td, th { border: 1px solid #cbd2d9; padding: 4px 8px; text-align: left; }
img { max-width: 100%; display: block; margin: 10px auto; page-break-inside: avoid; }
"""


def main():
    md = (DOCS / "ARCHITECTURE.md").read_text()
    body = markdown.markdown(md, extensions=["fenced_code", "tables"])
    html = (
        f"<!doctype html><html><head><meta charset='utf-8'><title>.dov pLang Architecture</title>"
        f"<base href='{DOCS.as_uri()}/'><style>{CSS}</style></head><body>{body}</body></html>"
    )
    out = DOCS / "ARCHITECTURE.pdf"
    with tempfile.TemporaryDirectory() as tmp:
        page = pathlib.Path(tmp) / "architecture.html"
        page.write_text(html)
        subprocess.run(
            [CHROME, "--headless", "--disable-gpu", "--no-pdf-header-footer",
             "--allow-file-access-from-files", f"--print-to-pdf={out}", page.as_uri()],
            check=True, capture_output=True,
        )
    print(f"wrote {out} ({out.stat().st_size} bytes)")


if __name__ == "__main__":
    sys.exit(main())
