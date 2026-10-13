"""Tests for the Sweetwater K2000 sounds fetcher (parsing only, no network)."""

from __future__ import annotations

from pathlib import Path

from fetch_kurzweil_k2000_sounds import Item, looks_like_html, parse, sanitize

SAMPLE_HTML = """
<nav><a href="#k2000-sounds-bass">Bass</a></nav>
<h3 class="x" id="k2000-sounds-bass">Bass</h3>
<ul><li><a href="https://example.com/files/bassman.krz">bassman.krz</a></li>
<li><a href="https://example.com/files/bassman.txt">bassman.txt</a></li></ul>
<h3 id="k2000-sounds-bells-chimes">Bells &amp; Chimes</h3>
<ul><li><a href="/files/ding.krz">ding.krz</a></li>
<li><a href="https://example.com/files/ding.krz">ding.krz</a></li></ul>
<h3 id="k2000-software-pc">PC</h3>
<a href="https://example.com/files/tool.zip">tool.zip</a>
"""


def test_parse_groups_by_section_and_ignores_non_krz() -> None:
    items = parse(SAMPLE_HTML, base_url="https://example.com/page/")
    assert items == [
        Item("Bass", "bassman.krz", "https://example.com/files/bassman.krz"),
        Item("Bells & Chimes", "ding.krz", "https://example.com/files/ding.krz"),
    ]


def test_parse_resolves_relative_links_and_deduplicates() -> None:
    items = parse(SAMPLE_HTML, base_url="https://example.com/page/")
    urls = [item.url for item in items]
    assert urls.count("https://example.com/files/ding.krz") == 1


def test_sanitize() -> None:
    assert sanitize("Bells & Chimes") == "Bells & Chimes"
    assert sanitize("a/b:c*d?") == "a_b_c_d_"
    assert sanitize("trailing. ") == "trailing"
    assert sanitize("") == "uncategorized"


def test_looks_like_html() -> None:
    assert looks_like_html(b"<!DOCTYPE html><html>...")
    assert looks_like_html(b"\n  <html lang=en>")
    assert looks_like_html(b"... px-captcha ...")
    assert not looks_like_html(b"PRAM\x00\x01")
    assert not looks_like_html(b"SROM\x00\x01")


def test_parse_real_page_if_available() -> None:
    page = Path(".tmp/sw.html")
    if not page.is_file():
        print("skipping: .tmp/sw.html not available")
        return
    items = parse(page.read_text(encoding="utf-8", errors="replace"))
    assert len(items) >= 200
    categories = {item.category for item in items}
    assert {"Bass", "Bells & Chimes", "Brass"} <= categories
    assert all(item.url.lower().endswith(".krz") for item in items)


def test_downloaded_files_if_available() -> None:
    directory = Path(".tmp/sweetwater")
    if not directory.is_dir():
        print("skipping: .tmp/sweetwater not available")
        return
    files = sorted(
        path
        for path in directory.rglob("*")
        if path.is_file() and path.suffix.lower() == ".krz"
    )
    if not files:
        print("skipping: no downloaded files")
        return
    for path in files:
        payload = path.read_bytes()
        assert not looks_like_html(payload), f"{path} is an HTML error page"
        assert payload[:4].isalpha(), f"{path} has no file magic"
