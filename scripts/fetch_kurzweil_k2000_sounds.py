#!/usr/bin/env python3
"""Download all ``.krz`` files from a Sweetwater Kurzweil K2000 sounds page.

Usage::

    uv run scripts/fetch_kurzweil_k2000_sounds.py <PAGE_URL> <TARGET_DIR> [options]

The page is parsed in document order: every ``<h3 id="k2000-sounds-...">`` heading
starts a category (``Bass``, ``Bells & Chimes``, ...) and each ``.krz`` link that
follows belongs to it. Categories become sub-directories of ``<TARGET_DIR>``.

Downloads are intentionally gentle: one request at a time, a pause between them,
retries with exponential backoff, and ``Retry-After`` support — so the source
server does not decide to ban us.
"""

from __future__ import annotations

import argparse
import gzip
import random
import re
import sys
import time
import urllib.error
import urllib.request
import zlib
from dataclasses import dataclass
from html import unescape
from pathlib import Path
from urllib.parse import unquote, urljoin, urlparse

DEFAULT_PAGE = (
    "https://www.sweetwater.com/sweetcare/kurzweil-k2000-sounds-and-software/"
)

USER_AGENT = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36"
)

PAGE_HEADERS = {
    "User-Agent": USER_AGENT,
    "Accept": "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8",
    "Accept-Language": "en-US,en;q=0.9",
    "Accept-Encoding": "gzip, deflate",
    "Connection": "close",
}

FILE_HEADERS = {
    "User-Agent": USER_AGENT,
    "Accept": "*/*",
    "Accept-Language": "en-US,en;q=0.9",
    "Accept-Encoding": "gzip, deflate",
    "Connection": "close",
}

_TAG = re.compile(r"<[^>]+>")
_TOKEN = re.compile(
    r'<h3[^>]*id="k2000-sounds-([^"]+)"[^>]*>(?P<heading>.*?)</h3>'
    r'|<a\s[^>]*href="(?P<href>[^"]+)"[^>]*>(?P<label>.*?)</a>',
    re.DOTALL | re.IGNORECASE,
)
_ILLEGAL = re.compile(r'[<>:"/\\|?*\x00-\x1f]')


@dataclass(frozen=True)
class Item:
    """One downloadable file and the category it belongs to."""

    category: str
    name: str
    url: str


def text_of(html_fragment: str) -> str:
    return unescape(_TAG.sub("", html_fragment)).strip()


def sanitize(name: str) -> str:
    """Make a string safe to use as a single path component."""
    cleaned = _ILLEGAL.sub("_", name)
    cleaned = cleaned.strip().rstrip(". ")
    return cleaned or "uncategorized"


def parse(html: str, base_url: str = "") -> list[Item]:
    """Parse ``.krz`` links grouped by their ``k2000-sounds-*`` section."""
    items: list[Item] = []
    category = ""
    seen: set[str] = set()

    for match in _TOKEN.finditer(html):
        heading = match.group("heading")
        if heading is not None:
            category = sanitize(text_of(heading))
            continue

        href = unescape(match.group("href") or "")
        if not href.lower().endswith(".krz"):
            continue

        url = urljoin(base_url, href) if base_url else href
        if url in seen:
            continue
        seen.add(url)

        name = unquote(Path(urlparse(url).path).name)
        if not name:
            continue
        items.append(Item(category or "uncategorized", name, url))

    return items


def decode_body(payload: bytes, encoding: str) -> bytes:
    encoding = (encoding or "").lower()
    if encoding == "gzip":
        return gzip.decompress(payload)
    if encoding == "deflate":
        try:
            return zlib.decompress(payload)
        except zlib.error:
            return zlib.decompress(payload, -zlib.MAX_WBITS)
    return payload


def fetch(url: str, headers: dict[str, str], timeout: float) -> bytes:
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=timeout) as response:
        payload = response.read()
        return decode_body(payload, response.headers.get("Content-Encoding", ""))


def load_page(page: str, timeout: float) -> str:
    """Read the page from http(s), a ``file://`` URL, or a local path."""
    if page.startswith(("http://", "https://")):
        return fetch(page, PAGE_HEADERS, timeout).decode("utf-8", "replace")

    if page.startswith("file://"):
        path = Path(urlparse(page).path)
    else:
        path = Path(page)
    return path.read_text(encoding="utf-8", errors="replace")


def retry_after(error: urllib.error.HTTPError) -> float | None:
    value = error.headers.get("Retry-After") if error.headers else None
    if not value:
        return None
    try:
        return float(value)
    except ValueError:
        return None


def looks_like_html(payload: bytes) -> bool:
    """True for error pages / bot walls that must not be saved as a sound file."""
    head = payload[:512].lstrip().lower()
    return (
        head.startswith((b"<!doctype html", b"<html"))
        or b"access to this page has been denied" in head
        or b"px-captcha" in head
    )


def download(
    item: Item,
    target_dir: Path,
    *,
    timeout: float,
    retries: int,
    delay: float,
    jitter: float,
    overwrite: bool,
) -> tuple[str, int]:
    """Download one file. Returns ``(status, size)``."""
    destination = target_dir / item.category / item.name

    if destination.exists() and destination.stat().st_size > 0 and not overwrite:
        return "skipped", destination.stat().st_size

    destination.parent.mkdir(parents=True, exist_ok=True)
    partial = destination.with_name(destination.name + ".part")

    last_error = "unknown"
    for attempt in range(retries + 1):
        try:
            payload = fetch(item.url, FILE_HEADERS, timeout)
        except urllib.error.HTTPError as error:
            last_error = f"HTTP {error.code}"
            if error.code in (429, 500, 502, 503, 504) and attempt < retries:
                wait = retry_after(error) or delay * (2**attempt)
                time.sleep(wait + random.uniform(0, jitter))
                continue
            return f"failed ({last_error})", 0
        except (urllib.error.URLError, TimeoutError, OSError) as error:
            last_error = str(error)
            if attempt < retries:
                time.sleep(delay * (2**attempt) + random.uniform(0, jitter))
                continue
            return f"failed ({last_error})", 0

        if looks_like_html(payload):
            return "failed (HTML response, not a sound file)", 0

        partial.write_bytes(payload)
        partial.replace(destination)
        return "downloaded", len(payload)

    return f"failed ({last_error})", 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("page", help=f"page URL (default content: {DEFAULT_PAGE})")
    parser.add_argument("target", type=Path, help="directory to download into")
    parser.add_argument(
        "--delay",
        type=float,
        default=1.5,
        help="base pause between requests in seconds (default: 1.5)",
    )
    parser.add_argument(
        "--jitter",
        type=float,
        default=0.5,
        help="random extra pause up to this many seconds (default: 0.5)",
    )
    parser.add_argument(
        "--retries", type=int, default=3, help="retries per file (default: 3)"
    )
    parser.add_argument(
        "--timeout", type=float, default=60.0, help="request timeout (default: 60)"
    )
    parser.add_argument(
        "--overwrite", action="store_true", help="re-download existing files"
    )
    parser.add_argument(
        "--list", action="store_true", help="only list what would be downloaded"
    )
    parser.add_argument(
        "--limit", type=int, default=0, help="stop after N files (0 = all)"
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)

    try:
        html = load_page(args.page, args.timeout)
    except (urllib.error.URLError, OSError) as error:
        print(f"error: cannot read page: {error}", file=sys.stderr)
        return 1

    items = parse(
        html, base_url=args.page if args.page.startswith(("http", "file")) else ""
    )
    if not items:
        print("error: no .krz links found on the page", file=sys.stderr)
        return 1

    print(f"found {len(items)} .krz files")
    if args.list:
        for item in items:
            print(f"  {item.category}/{item.name}\t{item.url}")
        return 0

    if args.limit:
        items = items[: args.limit]

    args.target.mkdir(parents=True, exist_ok=True)

    downloaded = skipped = failed = 0
    total = len(items)
    try:
        for index, item in enumerate(items, start=1):
            status, size = download(
                item,
                args.target,
                timeout=args.timeout,
                retries=args.retries,
                delay=args.delay,
                jitter=args.jitter,
                overwrite=args.overwrite,
            )
            detail = f"{status} ({size} bytes)" if size else status
            print(f"[{index}/{total}] {item.category}/{item.name}: {detail}")

            if status == "downloaded":
                downloaded += 1
            elif status == "skipped":
                skipped += 1
            else:
                failed += 1

            if status != "skipped":
                time.sleep(args.delay + random.uniform(0, args.jitter))
    except KeyboardInterrupt:
        print("\ninterrupted", file=sys.stderr)

    print(
        f"\ndone: {downloaded} downloaded, {skipped} skipped, {failed} failed "
        f"-> {args.target}"
    )
    return 1 if failed else 0


if __name__ == "__main__":
    raise SystemExit(main())
