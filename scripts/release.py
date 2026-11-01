#!/usr/bin/env python3
"""One version, five verified installers, one GitHub Release."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tomllib
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent
VERSION_FILES = (
    "agent/Cargo.toml", "Cargo.lock", "ui/RustClipSync/RustClipSync.csproj",
    "ui/RustClipSync/MainWindow.xaml", "agent/app.rc", "packaging/RustClipSync.iss",
    "ui/linux/CMakeLists.txt", "ui/linux/main.cpp", "packaging/linux/PKGBUILD",
)


def match_version(root, path, pattern):
    match = re.search(pattern, (root / path).read_text(encoding="utf-8-sig"))
    if not match:
        raise ValueError("Version metadata missing: " + path)
    return match.group(1)


def check_versions(root=ROOT):
    version = tomllib.loads((root / "agent/Cargo.toml").read_text())["package"]["version"]
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("A stable X.Y.Z version is required")
    locked = tomllib.loads((root / "Cargo.lock").read_text())["package"]
    values = {
        "Cargo.lock": next(p["version"] for p in locked if p["name"] == "rustclipsync-agent"),
        "WinUI assembly": ET.parse(root / "ui/RustClipSync/RustClipSync.csproj").findtext("PropertyGroup/Version"),
        "WinUI about": match_version(root, "ui/RustClipSync/MainWindow.xaml", r'Text="RustClipSync ([0-9.]+)"'),
        "installer": match_version(root, "packaging/RustClipSync.iss", r'#define AppVersion "([0-9.]+)"'),
        "Qt project": match_version(root, "ui/linux/CMakeLists.txt", r'project\(RustClipSyncLinux VERSION ([0-9.]+)'),
        "Qt application": match_version(root, "ui/linux/main.cpp", r'setApplicationVersion\("([0-9.]+)"\)'),
        "Arch package": match_version(root, "packaging/linux/PKGBUILD", r'pkgver=([0-9.]+)'),
        "Win32 FileVersion": match_version(root, "agent/app.rc", r'VALUE "FileVersion", "([0-9.]+)\\0"'),
        "Win32 ProductVersion": match_version(root, "agent/app.rc", r'VALUE "ProductVersion", "([0-9.]+)\\0"'),
    }
    for name, value in values.items():
        if value != version:
            raise ValueError(f"Version mismatch: {name}={value}, agent={version}")
    numeric = ",".join(version.split(".") + ["0"])
    resource = (root / "agent/app.rc").read_text()
    for field in ("FILEVERSION", "PRODUCTVERSION"):
        if not re.search(r"\b" + field + r"\s+" + re.escape(numeric) + r"\b", resource):
            raise ValueError("Win32 numeric version mismatch: " + field)
    return version


def expected_assets(version):
    return [
        f"RustClipSync-{version}-setup-x64.exe",
        f"rustclipsync-desktop-{version}-debian12-x86_64.deb",
        f"rustclipsync-desktop-{version}-ubuntu2404-x86_64.deb",
        f"rustclipsync-desktop-{version}-fedora43-x86_64.rpm",
        f"rustclipsync-desktop-{version}-arch-x86_64.pkg.tar.zst",
    ]


def digest(path):
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def checksums(path):
    result = {}
    for line in path.read_text(encoding="utf-8-sig").splitlines():
        if not line.strip():
            continue
        value, name = line.split(maxsplit=1)
        name = name.lstrip("*")
        if not re.fullmatch(r"[0-9a-fA-F]{64}", value) or name in result:
            raise ValueError("Invalid checksum manifest: " + str(path))
        result[name] = value.lower()
    return result


def prepare_assets(source, output, version):
    selected = []
    for name in expected_assets(version):
        matches = list(source.rglob(name))
        if len(matches) != 1:
            raise ValueError(f"Expected exactly one installer {name}, found {len(matches)}")
        path = matches[0]
        actual = digest(path)
        if path.stat().st_size == 0 or checksums(path.parent / "SHA256SUMS").get(name) != actual:
            raise ValueError("Installer checksum mismatch: " + name)
        selected.append((path, actual))
    if output.exists() and any(output.iterdir()):
        raise ValueError("Release staging directory must be empty")
    output.mkdir(parents=True, exist_ok=True)
    for path, _ in selected:
        shutil.copy2(path, output / path.name)
    (output / "SHA256SUMS").write_text("".join(f"{value}  {path.name}\n" for path, value in sorted(selected, key=lambda item: item[0].name)))
    return output


def run_gh(*arguments):
    subprocess.run(["gh", *arguments], check=True)


def api(path, method="GET", missing_ok=False, paginate=False):
    command = ["gh", "api", path, "--method", method]
    if paginate:
        command += ["--paginate", "--slurp"]
    result = subprocess.run(command, capture_output=True, text=True)
    if result.returncode:
        if missing_ok and "HTTP 404" in result.stderr:
            return None
        raise RuntimeError(result.stderr.strip())
    return json.loads(result.stdout) if result.stdout.strip() else None


def tag_commit(repo, tag):
    reference = api(f"repos/{repo}/git/ref/tags/{tag}", missing_ok=True)
    if reference is None:
        return None
    obj = reference["object"]
    for _ in range(8):
        if obj["type"] == "commit":
            return obj["sha"]
        if obj["type"] != "tag":
            break
        obj = api(f"repos/{repo}/git/tags/{obj['sha']}")["object"]
    raise ValueError("Release tag does not point to a commit")


def find_release(repo, tag):
    # Listing with push access includes drafts whose tag is not published yet.
    matches = [release for page in api(f"repos/{repo}/releases?per_page=100", paginate=True)
               for release in page if release["tag_name"] == tag]
    if len(matches) > 1:
        raise ValueError("Multiple releases use the same version tag")
    return matches[0] if matches else None


def plan(version, environment, root=ROOT):
    event, ref = environment.get("GITHUB_EVENT_NAME", ""), environment.get("GITHUB_REF", "")
    tagged = event == "push" and ref.startswith("refs/tags/")
    requested = event == "workflow_dispatch" and environment.get("REQUEST_PUBLISH") == "true"
    if tagged and ref != "refs/tags/v" + version:
        raise ValueError("The pushed tag does not match the application version")
    if requested and ref not in ("refs/heads/main", "refs/tags/v" + version):
        raise ValueError("Publish from main or its matching version tag")
    notes = root / "releases" / f"v{version}.md"
    automatic = event == "push" and ref == "refs/heads/main" and notes.is_file()
    publish = False
    if tagged or requested or automatic:
        existing = tag_commit(environment["GITHUB_REPOSITORY"], "v" + version)
        publish = tagged or requested or existing is None
        if publish and existing is not None and existing != environment["GITHUB_SHA"]:
            raise ValueError("The version tag already belongs to another commit")
        if publish and (not notes.is_file() or not notes.read_text().strip()):
            raise ValueError("Release notes are required")
    return publish


def publish_assets(directory, version, environment):
    repo, sha = environment["GITHUB_REPOSITORY"], environment["GITHUB_SHA"]
    tag = "v" + version
    names = expected_assets(version) + ["SHA256SUMS"]
    if {p.name for p in directory.iterdir()} != set(names):
        raise ValueError("Release must contain all five installers and SHA256SUMS")
    manifest = checksums(directory / "SHA256SUMS")
    if set(manifest) != set(expected_assets(version)):
        raise ValueError("Incomplete release checksum manifest")
    for name in expected_assets(version):
        if manifest[name] != digest(directory / name):
            raise ValueError("Staged release checksum mismatch: " + name)
    commit = tag_commit(repo, tag)
    if commit is not None and commit != sha:
        raise ValueError("Refusing to replace an existing version tag")
    release = find_release(repo, tag)
    files = [str(directory / name) for name in names]
    if release is not None:
        if not release["draft"]:
            if commit == sha and {a["name"] for a in release["assets"]} == set(names):
                print("Release already published: " + release["html_url"])
                return
            raise ValueError("Refusing to change a published release")
        if release["target_commitish"] != sha:
            raise ValueError("Draft release targets another commit")
        run_gh("release", "upload", tag, *files, "--clobber", "--repo", repo)
    else:
        run_gh("release", "create", tag, *files, "--draft", "--target", sha,
               "--title", "RustClipSync " + version, "--notes-file", str(ROOT / "releases" / f"{tag}.md"), "--repo", repo)
    release = find_release(repo, tag)
    if release is None:
        raise ValueError("The draft release was not created")
    endpoint = f"repos/{repo}/releases/{release['id']}"
    release = api(endpoint)
    actual = {asset["name"]: asset["size"] for asset in release["assets"]}
    expected = {name: (directory / name).stat().st_size for name in names}
    if not release["draft"] or actual != expected:
        raise ValueError("Draft assets are incomplete or have unexpected sizes")
    run_gh("release", "edit", tag, "--draft=false", "--latest", "--target", sha, "--repo", repo)
    published = api(endpoint)
    if published["draft"] or published["prerelease"] or tag_commit(repo, tag) != sha:
        raise ValueError("Published release verification failed")
    print("Published: " + published["html_url"])


def cleanup_failed(environment):
    repo = environment["GITHUB_REPOSITORY"]
    current = int(environment["GITHUB_RUN_ID"])
    runs = {}
    for conclusion in ("failure", "timed_out"):
        for page in api(f"repos/{repo}/actions/runs?status={conclusion}&per_page=100", paginate=True):
            for run in page["workflow_runs"]:
                runs[run["id"]] = run
    removed, artifacts = [], 0
    for identity in sorted(runs):
        if identity == current:
            continue
        latest = api(f"repos/{repo}/actions/runs/{identity}", missing_ok=True)
        if latest is None or latest["status"] != "completed" or latest["conclusion"] not in ("failure", "timed_out"):
            continue
        for page in api(f"repos/{repo}/actions/runs/{identity}/artifacts?per_page=100", paginate=True):
            for artifact in page["artifacts"]:
                api(f"repos/{repo}/actions/artifacts/{artifact['id']}", "DELETE", missing_ok=True)
                artifacts += 1
        api(f"repos/{repo}/actions/runs/{identity}", "DELETE", missing_ok=True)
        removed.append(identity)
    result = f"Cleaned {len(removed)} failed/timed-out workflow runs and {artifacts} artifacts."
    print(result)
    print("Deleted run IDs: " + ", ".join(map(str, removed)))
    if environment.get("GITHUB_STEP_SUMMARY"):
        with open(environment["GITHUB_STEP_SUMMARY"], "a") as summary:
            summary.write(result + "\n")


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("check")
    check.add_argument("--github-output", type=Path)
    prepare = sub.add_parser("prepare")
    prepare.add_argument("--artifacts", type=Path, required=True)
    prepare.add_argument("--output", type=Path, required=True)
    publish = sub.add_parser("publish")
    publish.add_argument("--assets", type=Path, required=True)
    sub.add_parser("cleanup")
    arguments = parser.parse_args()
    if arguments.command == "cleanup":
        cleanup_failed(os.environ)
        return
    version = check_versions()
    if arguments.command == "check":
        publish = plan(version, os.environ) if arguments.github_output else False
        print(f"Version {version}; publish={str(publish).lower()}")
        if arguments.github_output:
            with arguments.github_output.open("a") as output:
                output.write(f"version={version}\npublish={str(publish).lower()}\n")
    elif arguments.command == "prepare":
        prepare_assets(arguments.artifacts, arguments.output, version)
    elif arguments.command == "publish":
        publish_assets(arguments.assets, version, os.environ)


if __name__ == "__main__":
    main()
