#!/usr/bin/env python3
"""
FlowDownload Desktop Installer — Phase 1 CLI (Windows + macOS)
Zero-dependency core (Python 3.10+ stdlib) + optional pymobiledevice3 for USB discovery.
"""

import argparse
import hashlib
import json
import os
import plistlib
import shutil
import sys
import urllib.request
import zipfile
from pathlib import Path

DEFAULT_SOURCE_URL = "https://muhammad-favel.github.io/FlowDownload-Releases/source.json"

PROFILES = {
    "full": {
        "app_ids": 3,
        "excluded_appex": set(),
        "kept": [
            "Wi-Fi + Cellular Boost (HTTP Range + libtorrent)",
            "Native C++ libtorrent engine",
            "Built-in AdGuard content-blocking browser",
            "External USB drive staging & export",
            "Automatic SHA-256 verification",
            "Safari toolbar extension & Share Sheet handoff",
        ],
        "trade_offs": [
            "Uses all 3 App IDs on a free Apple ID (cannot coexist with other sideloaded apps)."
        ],
    },
    "share-only": {
        "app_ids": 2,
        "excluded_appex": {"FlowSafariExtension.appex"},
        "kept": [
            "All core FlowDownload features (Boost, libtorrent, AdGuard browser, USB export, SHA-256)",
            "System-wide Share Sheet handoff (Safari / Files -> Share -> FlowDownload)",
        ],
        "trade_offs": [
            "Loses the Safari toolbar popup/extension button.",
            "Requires 2 free App ID slots on a free Apple ID.",
        ],
    },
    "core": {
        "app_ids": 1,
        "excluded_appex": {"FlowSafariExtension.appex", "FlowShareExtension.appex"},
        "kept": [
            "Wi-Fi + Cellular Boost (HTTP Range + libtorrent)",
            "Native C++ libtorrent engine (magnet & .torrent files)",
            "Built-in AdGuard content-blocking browser",
            "External USB drive staging & export",
            "Automatic SHA-256 verification",
        ],
        "trade_offs": [
            "Loses the Safari toolbar extension.",
            "Loses Share Sheet handoff from other apps (paste links in FlowDownload, open .torrent from Files, or use the built-in browser instead).",
        ],
    },
}


def fetch_latest_release(source_url: str) -> tuple[dict, dict]:
    req = urllib.request.Request(source_url, headers={"User-Agent": "FlowDownload-Installer/0.1"})
    with urllib.request.urlopen(req, timeout=20) as resp:
        data = json.loads(resp.read().decode("utf-8"))
    app = data["apps"][0]
    latest = app["versions"][0]
    return app, latest


def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()


def download_ipa(version: dict, cache_dir: Path) -> Path:
    cache_dir.mkdir(parents=True, exist_ok=True)
    dest = cache_dir / f"FlowDownload-v{version['version']}.ipa"
    expected_sha = version.get("sha256")
    expected_size = version.get("size")

    if dest.exists():
        if expected_sha and sha256_file(dest).lower() == expected_sha.lower():
            print(f"Using cached IPA (SHA-256 verified): {dest}")
            return dest
        if not expected_sha and expected_size and dest.stat().st_size == expected_size:
            print(f"Using cached IPA: {dest}")
            return dest

    url = version["downloadURL"]
    print(f"Downloading {url} ({(expected_size or 0) / 1_000_000:.1f} MB)...")
    tmp = dest.with_suffix(".ipa.part")
    req = urllib.request.Request(url, headers={"User-Agent": "FlowDownload-Installer/0.1"})
    h = hashlib.sha256()
    with urllib.request.urlopen(req, timeout=120) as resp, tmp.open("wb") as out:
        while chunk := resp.read(65536):
            out.write(chunk)
            h.update(chunk)

    actual_sha = h.hexdigest()
    if expected_sha and actual_sha.lower() != expected_sha.lower():
        tmp.unlink(missing_ok=True)
        raise RuntimeError(f"SHA-256 mismatch: expected {expected_sha}, got {actual_sha}")

    if expected_sha:
        print(f"SHA-256 verified: {actual_sha}")
    shutil.move(str(tmp), str(dest))
    return dest


def inspect_ipa(ipa_path: Path) -> dict:
    bundle_name = "FlowDownload.app"
    bundle_id = None
    plugins = set()
    with zipfile.ZipFile(ipa_path, "r") as zf:
        for name in zf.namelist():
            if not name.startswith("Payload/"):
                continue
            rest = name[len("Payload/"):]
            idx = rest.find(".app/")
            if idx == -1:
                continue
            bundle_name = rest[: idx + 4]
            inner = rest[idx + 5 :]
            if inner == "Info.plist":
                with zf.open(name) as pf:
                    info = plistlib.load(pf)
                    bundle_id = info.get("CFBundleIdentifier")
            elif inner.startswith("PlugIns/"):
                plug_rest = inner[len("PlugIns/"):]
                appex_idx = plug_rest.find(".appex/")
                if appex_idx != -1:
                    plugins.add(plug_rest[: appex_idx + 6])
    return {
        "app_bundle_name": bundle_name,
        "bundle_id": bundle_id,
        "plugins": sorted(plugins),
    }


def prepare_ipa_for_profile(ipa_path: Path, profile_name: str, work_dir: Path) -> Path:
    excluded = PROFILES[profile_name]["excluded_appex"]
    if not excluded:
        return ipa_path

    work_dir.mkdir(parents=True, exist_ok=True)
    suffix = {"share-only": "ShareOnly", "core": "Core"}[profile_name]
    out_path = work_dir / f"FlowDownload-{suffix}.ipa"

    with zipfile.ZipFile(ipa_path, "r") as zin, zipfile.ZipFile(out_path, "w") as zout:
        for item in zin.infolist():
            skip = any(
                f"/PlugIns/{appex}/" in item.filename or item.filename.endswith(f"/PlugIns/{appex}")
                for appex in excluded
            )
            if skip:
                continue
            zout.writestr(item, zin.read(item.filename))
    return out_path


def print_trade_offs() -> None:
    print("\n=== FlowDownload App ID Budget & Trade-Offs ===")
    print("Free Apple IDs allow up to 3 active App IDs per device (10 per 7 days).\n")
    for name, info in PROFILES.items():
        print(f"[{name}] - {info['app_ids']} App ID(s)")
        if info["excluded_appex"]:
            print(f"  Dropped: {', '.join(sorted(info['excluded_appex']))}")
        for t in info["trade_offs"]:
            print(f"  Trade-off: {t}")
        print()


def _usbmux_list_devices_stdlib() -> tuple[list[dict], str | None]:
    import socket
    import struct

    sock = None
    try:
        if sys.platform == "win32":
            sock = socket.create_connection(("127.0.0.1", 27015), timeout=3)
        else:
            sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
            sock.settimeout(3)
            sock.connect("/var/run/usbmuxd")
    except OSError as exc:
        if sys.platform == "win32":
            return [], (
                f"Apple Mobile Device Service not reachable on 127.0.0.1:27015 ({exc}). "
                "Install Apple Devices or iTunes on Windows for USB driver support."
            )
        return [], f"usbmuxd not reachable ({exc})."

    try:
        payload = plistlib.dumps(
            {
                "MessageType": "ListDevices",
                "ProgName": "FlowDownload-Installer",
                "ClientVersionString": "0.1",
            }
        )
        header = struct.pack("<IIII", 16 + len(payload), 1, 8, 1)
        sock.sendall(header + payload)

        raw_hdr = sock.recv(16)
        if len(raw_hdr) < 16:
            return [], "Short usbmuxd response header."
        total_len, _ver, _msg, _tag = struct.unpack("<IIII", raw_hdr)
        body = bytearray()
        while len(body) < total_len - 16:
            chunk = sock.recv(total_len - 16 - len(body))
            if not chunk:
                break
            body.extend(chunk)
        resp = plistlib.loads(bytes(body))
        return resp.get("DeviceList", []), None
    finally:
        sock.close()


def list_usb_devices() -> None:
    devices, err = _usbmux_list_devices_stdlib()
    if err:
        print(err)
        return
    if not devices:
        print("No iOS USB devices detected. Connect an iPhone via USB and tap 'Trust This Computer'.")
        return
    for dev in devices:
        props = dev.get("Properties", {})
        print(
            f"UDID: {props.get('SerialNumber', 'unknown')}  "
            f"Connection: {props.get('ConnectionType', 'USB')}  "
            f"DeviceID: {dev.get('DeviceID')}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(description="FlowDownload Desktop Installer CLI (Windows + macOS)")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_check = sub.add_parser("check", help="Fetch latest release info from source.json")
    p_check.add_argument("--source", default=DEFAULT_SOURCE_URL)

    sub.add_parser("trade-offs", help="Show 3-App-ID / 2-App-ID / 1-App-ID trade-offs")

    sub.add_parser("devices", help="List connected iOS devices over USB")

    p_prep = sub.add_parser("prepare", help="Download, verify SHA-256, inspect, and package IPA for a profile")
    p_prep.add_argument("--source", default=DEFAULT_SOURCE_URL)
    p_prep.add_argument("--profile", choices=list(PROFILES.keys()), default="full")
    p_prep.add_argument("--cache-dir", default=".flowdownload-cache")

    args = parser.parse_args()

    if args.cmd == "check":
        app, latest = fetch_latest_release(args.source)
        print(f"{app['name']} ({app['bundleIdentifier']})")
        print(f"  Latest: v{latest['version']} ({latest['date']})")
        print(f"  Size:   {latest['size'] / 1_000_000:.1f} MB")
        print(f"  URL:    {latest['downloadURL']}")
        if latest.get("sha256"):
            print(f"  SHA256: {latest['sha256']}")
    elif args.cmd == "trade-offs":
        print_trade_offs()
    elif args.cmd == "devices":
        list_usb_devices()
    elif args.cmd == "prepare":
        _, latest = fetch_latest_release(args.source)
        cache_dir = Path(args.cache_dir)
        ipa_path = download_ipa(latest, cache_dir)
        info = inspect_ipa(ipa_path)
        print(f"Inspected {info['app_bundle_name']} ({info['bundle_id']}): {len(info['plugins'])} extension(s) {info['plugins']}")

        p = PROFILES[args.profile]
        print(f"\nSelected profile: {args.profile} ({p['app_ids']} App ID(s))")
        for t in p["trade_offs"]:
            print(f"  Note: {t}")

        out_ipa = prepare_ipa_for_profile(ipa_path, args.profile, cache_dir)
        out_info = inspect_ipa(out_ipa)
        print(f"Prepared: {out_ipa} ({out_ipa.stat().st_size / 1_000_000:.1f} MB), extensions={out_info['plugins']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
