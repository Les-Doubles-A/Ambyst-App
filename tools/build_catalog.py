"""Builds catalog.json: every public repository of OWNER with its latest release
and the list of its image files, so that Ambyst-App needs a single download
(raw.githubusercontent.com, outside the GitHub API rate limit) instead of one
API call per repository.

Usage: GITHUB_TOKEN=... python tools/build_catalog.py OWNER OUTPUT.json

The output is deterministic (no timestamp), so it only changes when a
repository, a release or an image changes.
"""

import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.github.com"
MAX_IMAGES = 300


def get(path):
    """GET an API path. Returns parsed JSON, or None on 404 / 409 (empty repository)."""
    req = urllib.request.Request(API + path, headers={
        "Accept": "application/vnd.github+json",
        "User-Agent": "Ambyst-App-catalog",
        "X-GitHub-Api-Version": "2022-11-28",
    })
    token = os.environ.get("GITHUB_TOKEN", "").strip()
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        if e.code in (404, 409):
            return None
        raise


def list_repos(owner):
    repos, page = [], 1
    while True:
        batch = get(f"/users/{owner}/repos?per_page=100&page={page}&sort=full_name") or []
        repos += batch
        if len(batch) < 100:
            return repos
        page += 1


def latest_release(full_name):
    rel = get(f"/repos/{full_name}/releases/latest")
    if not rel:
        return None
    return {
        "tag_name": rel["tag_name"],
        "html_url": rel["html_url"],
        "assets": [
            {"name": a["name"], "browser_download_url": a["browser_download_url"], "size": a["size"]}
            for a in sorted(rel.get("assets", []), key=lambda a: a["name"])
        ],
    }


def images(full_name, branch):
    branch_q = urllib.parse.quote(branch, safe="")
    tree = get(f"/repos/{full_name}/git/trees/{branch_q}?recursive=1")
    if not tree:
        return []
    paths = [
        e["path"] for e in tree.get("tree", [])
        if e.get("type") == "blob" and e["path"].lower().endswith((".png", ".ico"))
        and not any(d in e["path"].lower() for d in ("node_modules/", "target/"))
    ]
    return sorted(paths)[:MAX_IMAGES]


def main():
    owner, out = sys.argv[1], sys.argv[2]
    entries = []
    for r in sorted(list_repos(owner), key=lambda r: r["full_name"].lower()):
        if r.get("fork") or r.get("archived"):
            continue
        full = r["full_name"]
        entries.append({
            "full_name": full,
            "name": r["name"],
            "description": r.get("description") or "",
            "default_branch": r["default_branch"],
            "pushed_at": r.get("pushed_at") or "",
            "release": latest_release(full),
            "images": images(full, r["default_branch"]),
        })
        print(full, file=sys.stderr)
    with open(out, "w", encoding="utf-8", newline="\n") as f:
        json.dump({"version": 1, "repos": entries}, f, ensure_ascii=False, indent=1, sort_keys=True)
        f.write("\n")


if __name__ == "__main__":
    main()
