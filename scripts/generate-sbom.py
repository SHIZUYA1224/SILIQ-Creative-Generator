#!/usr/bin/env python3
"""Development-only offline dependency inventory and bundled license collector.

Cargo metadata supplies identity/license/normal edges; cargo tree restricts the
result to the selected package, target and runtime feature context. Neither
registry source paths nor credentials are written to the resulting CycloneDX.
"""
from __future__ import annotations

import argparse
import base64
import hashlib
import json
import pathlib
import re
import subprocess
import tarfile
import tomllib
import urllib.parse
import uuid

ROOT = pathlib.Path(__file__).resolve().parents[1]
MAX_LICENSE = 1024 * 1024


def run(*args: str) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True, stderr=subprocess.PIPE)


def host_target() -> str:
    return next(line.removeprefix("host: ") for line in run("rustc", "-vV").splitlines() if line.startswith("host: "))


def runtime_graph(package: str, target: str) -> tuple[dict, dict, dict, dict]:
    release_features = ("--features", "siliq-desktop/custom-protocol") if package == "siliq-desktop" else ()
    metadata = json.loads(run("cargo", "metadata", "--locked", "--offline", "--format-version", "1", "--filter-platform", target, *release_features))
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    root = next(p for p in packages.values() if p["name"] == package and p["id"] in metadata["workspace_members"])
    tree = run("cargo", "tree", "--locked", "--offline", "--target", target, "-p", package, "-e", "normal", "--prefix", "none", "--format", "{p}|{f}", *release_features)
    features = {}
    for line in tree.splitlines():
        match = re.fullmatch(r"([A-Za-z0-9_-]+) v([^ ]+)(?: \([^|]+\))?\|([^*]*?)(?: \(\*\))?", line)
        if match:
            name, version, flags = match.groups()
            features.setdefault((name, version), set()).update(f for f in flags.strip().split(",") if f)
    reached, pending = set(), [root["id"]]
    while pending:
        package_id = pending.pop()
        if package_id in reached:
            continue
        p = packages[package_id]
        if (p["name"], p["version"]) not in features:
            continue
        reached.add(package_id)
        pending.extend(d["pkg"] for d in nodes[package_id]["deps"] if any(k["kind"] is None for k in d["dep_kinds"]))
    return root, {key: packages[key] for key in reached}, nodes, features


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def source_fingerprint(package: dict) -> str:
    directory = pathlib.Path(package["manifest_path"]).parent
    files = [directory / "Cargo.toml"]
    src = directory / "src"
    if src.is_dir():
        files += sorted(src.rglob("*.rs"))
    hasher = hashlib.sha256()
    for file in files:
        if file.is_symlink() or not file.is_file():
            continue
        relative = file.relative_to(directory).as_posix().encode()
        hasher.update(len(relative).to_bytes(4, "little")); hasher.update(relative)
        contents = file.read_bytes()
        hasher.update(len(contents).to_bytes(8, "little")); hasher.update(contents)
    return hasher.hexdigest()


def license_texts(directory: pathlib.Path, declared: str | None = None) -> list[pathlib.Path]:
    candidates = [p for p in directory.iterdir() if p.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "COPYRIGHT", "NOTICE"))]
    if declared:
        candidates.append(directory / declared)
    result = []
    for path in sorted(set(candidates)):
        if path.is_symlink() or not path.is_file() or not path.resolve().is_relative_to(directory.resolve()):
            continue
        if path.stat().st_size > MAX_LICENSE:
            raise ValueError("dependency license text exceeds the limit")
        result.append(path)
    return result


def copy_licenses(paths: list[pathlib.Path], destination: pathlib.Path) -> list[dict]:
    copied = []
    for source in paths:
        destination.mkdir(parents=True, exist_ok=True)
        name = re.sub(r"[^A-Za-z0-9._-]", "_", source.name)
        data = source.read_bytes()
        output = destination / name
        output.write_bytes(data)
        copied.append({"file": output.relative_to(ROOT).as_posix(), "sha256": digest(data)})
    return copied


def licenses(expression: str | None) -> list[dict]:
    if not expression:
        return [{"license": {"name": "License text supplied by upstream"}}]
    if expression == "UNLICENSED":
        return [{"license": {"name": "UNLICENSED — first-party repository policy unchanged"}}]
    # Older upstream package metadata uses slash-separated declarations that
    # are not SPDX expressions. Preserve their exact text as a named license.
    if "/" in expression:
        return [{"license": {"name": expression}}]
    return [{"expression": expression}]


def native_lock(target: str) -> dict:
    for line in (ROOT / "third_party/skia-binaries.lock").read_text().splitlines():
        if line.startswith(target + " "):
            triple, release, key, checksum = line.split()
            return {"target": triple, "release": release, "key": key, "sha256": checksum}
    raise ValueError("no checksum-pinned Skia archive for this target")


def npm_runtime(license_dir: pathlib.Path) -> tuple[list[dict], list[dict], list[str], list[dict]]:
    lock = json.loads((ROOT / "package-lock.json").read_text())
    entries = lock["packages"]
    root = entries[""]
    pending = ["node_modules/" + name for name in root.get("dependencies", {})]
    reached = set(); components = []; notices = []; missing = []; graph = []
    def resolve(parent: str, name: str) -> str:
        path = pathlib.PurePosixPath(parent)
        for ancestor in (path, *path.parents):
            candidate = str(ancestor / "node_modules" / name)
            if candidate.startswith("./"):
                candidate = candidate[2:]
            if candidate in entries:
                return candidate
        candidate = "node_modules/" + name
        if candidate not in entries:
            raise ValueError("a frontend runtime dependency is absent from the lockfile")
        return candidate
    while pending:
        key = pending.pop()
        if key in reached:
            continue
        if not key.startswith("node_modules/") or ".." in pathlib.PurePosixPath(key).parts:
            raise ValueError("nonportable npm dependency path")
        reached.add(key)
        entry = entries[key]
        directory = ROOT / key
        if directory.is_symlink() or not directory.is_dir():
            raise ValueError("run npm ci before collecting runtime licenses")
        package = json.loads((directory / "package.json").read_text())
        name, version = package["name"], package["version"]
        expression = package.get("license") or entry.get("license")
        if isinstance(expression, dict):
            expression = expression.get("type")
        purl = "pkg:npm/" + urllib.parse.quote(name, safe="/") + "@" + urllib.parse.quote(version, safe="")
        hashes = []
        for token in entry.get("integrity", "").split():
            if token.startswith(("sha256-", "sha384-", "sha512-")):
                algorithm, value = token.split("-", 1)
                hashes.append({"alg": "SHA-" + algorithm[3:], "content": base64.b64decode(value, validate=True).hex()})
        components.append({"type": "library", "bom-ref": purl, "name": name, "version": version, "purl": purl, "licenses": licenses(expression), "hashes": hashes})
        copied = copy_licenses(license_texts(directory), license_dir / "npm" / (re.sub(r"[^A-Za-z0-9._-]", "_", name) + "-" + version))
        if not copied:
            missing.append("npm:" + name + "@" + version)
        notices.append({"ecosystem": "npm", "name": name, "version": version, "license": expression or "see upstream text", "files": copied})
        dependencies = {**entry.get("dependencies", {}), **entry.get("optionalDependencies", {})}
        dependency_keys = [resolve(key, name) for name in dependencies]
        dependency_refs = []
        for dependency_key in dependency_keys:
            dependency = json.loads((ROOT / dependency_key / "package.json").read_text())
            dependency_refs.append("pkg:npm/" + urllib.parse.quote(dependency["name"], safe="/") + "@" + urllib.parse.quote(dependency["version"], safe=""))
        graph.append({"ref": purl, "dependsOn": sorted(set(dependency_refs))})
        pending.extend(dependency_keys)
    return components, notices, missing, graph


def generate(package: str, target: str, output: pathlib.Path, license_dir: pathlib.Path) -> None:
    for provenance_name in ("native-license-sources.json", "upstream-license-sources.json"):
        provenance = json.loads((ROOT / "third_party" / provenance_name).read_text())
        for entry in provenance.get("licenses", provenance.get("sources", [])):
            path = (ROOT / entry["file"]).resolve()
            if not path.is_relative_to((ROOT / "third_party/licenses").resolve()) or digest(path.read_bytes()) != entry["sha256"]:
                raise ValueError("an upstream license snapshot does not match its provenance hash")
    root, packages, nodes, features = runtime_graph(package, target)
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())
    checksums = {(p["name"], p["version"]): p.get("checksum") for p in lock["package"]}
    if any(p["name"]=="selectors" for p in packages.values()):
        corresponding_source = ROOT / "third_party/sources/selectors-0.38.0.crate"
        if digest(corresponding_source.read_bytes()) != checksums[("selectors", "0.38.0")]:
            raise ValueError("MPL corresponding source does not match Cargo.lock")
    components = []; notices = []; missing = []; dependencies = []
    fallback_path = ROOT / "third_party/license-fallbacks.json"
    fallbacks = json.loads(fallback_path.read_text()).get("packages", {}) if fallback_path.exists() else {}
    refs = {key: f"pkg:cargo/{p['name']}@{p['version']}" for key, p in packages.items()}
    for package_id, p in sorted(packages.items(), key=lambda item: (item[1]["name"], item[1]["version"])):
        purl = refs[package_id]
        checksum = checksums.get((p["name"], p["version"])) or source_fingerprint(p)
        component = {"type": "library", "bom-ref": purl, "name": p["name"], "version": p["version"], "purl": purl,
                     "licenses": licenses(p["license"]), "hashes": [{"alg": "SHA-256", "content": checksum}],
                     "properties": [{"name": "siliq:source", "value": "crates.io" if p["source"] else "workspace"},
                                    {"name": "siliq:runtime-features", "value": ",".join(sorted(features[(p["name"], p["version"])]))},
                                    {"name": "siliq:compile-role", "value": "proc-macro" if any("proc-macro" in t["kind"] for t in p["targets"]) else "library"}]}
        components.append(component)
        dependencies.append({"ref": purl, "dependsOn": sorted({refs[d["pkg"]] for d in nodes[package_id]["deps"] if d["pkg"] in refs and any(k["kind"] is None for k in d["dep_kinds"])})})
        if p["source"]:
            directory = pathlib.Path(p["manifest_path"]).parent
            paths = license_texts(directory, p["license_file"])
            if not paths and p["name"] in ("skia-safe", "skia-bindings"):
                paths = [ROOT / "third_party/licenses/native/rust-skia-MIT.txt"]
            if not paths:
                for relative in fallbacks.get("cargo:" + p["name"] + "@" + p["version"], []):
                    path = (ROOT / relative).resolve()
                    if not path.is_relative_to((ROOT / "third_party/licenses").resolve()) or not path.is_file():
                        raise ValueError("invalid local upstream-license fallback")
                    paths.append(path)
            copied = copy_licenses(paths, license_dir / "cargo" / (p["name"] + "-" + p["version"]))
            if not copied:
                missing.append("cargo:" + p["name"] + "@" + p["version"])
            notices.append({"ecosystem": "cargo", "name": p["name"], "version": p["version"], "license": p["license"] or "see upstream text", "files": copied})
    native = native_lock(target)
    archive = ROOT / ".skia-cache" / ("skia-binaries-" + native["key"] + ".tar.gz")
    if not archive.is_file() or digest(archive.read_bytes()) != native["sha256"]:
        raise ValueError("prepare the verified native Skia archive before collecting licenses")
    with tarfile.open(archive, "r:gz") as tar:
        source = tar.extractfile("skia-binaries/LICENSE_SKIA")
        if source is None:
            raise ValueError("verified Skia archive has no native license")
        data = source.read(MAX_LICENSE + 1)
        if len(data) > MAX_LICENSE:
            raise ValueError("native license exceeds the limit")
        destination = license_dir / "native/Skia-BSD-3-Clause.txt"
        destination.parent.mkdir(parents=True, exist_ok=True); destination.write_bytes(data)
    native_ref = "pkg:generic/skia@m153-0.101.2"
    components.append({"type": "library", "bom-ref": native_ref, "name": "Skia", "version": "m153-0.101.2", "purl": native_ref,
                       "licenses": licenses("BSD-3-Clause"), "hashes": [{"alg": "SHA-256", "content": native["sha256"]}],
                       "properties": [{"name": "siliq:archive-target", "value": target}, {"name": "siliq:rust-skia-release", "value": native["release"]}, {"name": "siliq:archive-key", "value": native["key"]}]})
    for edge in dependencies:
        if edge["ref"].startswith("pkg:cargo/skia-bindings@"):
            edge["dependsOn"].append(native_ref)
    if package == "siliq-desktop":
        npm_components, npm_notices, npm_missing, npm_dependencies = npm_runtime(license_dir)
        components += npm_components; notices += npm_notices; missing += npm_missing
        dependencies += npm_dependencies
        direct_frontend = set(json.loads((ROOT / "package.json").read_text())["dependencies"])
        for edge in dependencies:
            if edge["ref"] == refs[root["id"]]:
                edge["dependsOn"] += sorted(c["bom-ref"] for c in npm_components if c["name"] in direct_frontend)
    lock_hash = digest((ROOT / "Cargo.lock").read_bytes() + (ROOT / "package-lock.json").read_bytes() + target.encode())
    root_component = next(c for c in components if c["bom-ref"] == refs[root["id"]])
    if package == "siliq-native-bridge":
        gui_hash = hashlib.sha256()
        for file in sorted((ROOT / "apps/macos/Sources").glob("*.swift")):
            name = file.name.encode(); content = file.read_bytes()
            gui_hash.update(len(name).to_bytes(4, "little")); gui_hash.update(name)
            gui_hash.update(len(content).to_bytes(8, "little")); gui_hash.update(content)
        gui_ref = "pkg:generic/siliq-macos-ui@0.1.0"
        components.append({"type":"application","bom-ref":gui_ref,"purl":gui_ref,
                           "name":"SILIQ native macOS UI","version":"0.1.0",
                           "licenses":[{"license":{"name":"UNLICENSED (workspace)"}}],
                           "hashes":[{"alg":"SHA-256","content":gui_hash.hexdigest()}],
                           "properties":[{"name":"siliq:source","value":"workspace Swift sources"},
                                         {"name":"siliq:system-frameworks","value":"SwiftUI, AppKit, Foundation, UniformTypeIdentifiers; macOS system-provided"},
                                         {"name":"siliq:minimum-macos","value":"13.0"}]})
        dependencies.append({"ref":gui_ref,"dependsOn":[root_component["bom-ref"]]})
        # Preserve the linked Rust root as a dependency of the GUI application.
        root_component = components[-1]
    root_component["type"] = "application"
    components = [c for c in components if c["bom-ref"] != root_component["bom-ref"]]
    bom = {"$schema": "http://cyclonedx.org/schema/bom-1.6.schema.json", "bomFormat": "CycloneDX", "specVersion": "1.6", "serialNumber": "urn:uuid:" + str(uuid.UUID(lock_hash[:32])), "version": 1,
           "metadata": {"component": root_component, "properties": [{"name": "siliq:target", "value": target}, {"name": "siliq:cargo-lock-sha256", "value": digest((ROOT / "Cargo.lock").read_bytes())}, {"name": "siliq:npm-lock-sha256", "value": digest((ROOT / "package-lock.json").read_bytes())}, {"name": "siliq:coverage", "value": "selected target normal Cargo closure; native Swift source when selected; React runtime npm closure only for Tauri target; native Skia archive; system frameworks and native vendored libraries require separate provenance review"}]},
           "components": sorted(components, key=lambda c: c["bom-ref"]), "dependencies": sorted(dependencies, key=lambda d: d["ref"])}
    serialized = json.dumps(bom, ensure_ascii=False, indent=2) + "\n"
    if str(ROOT) in serialized or "/Users/" in serialized or "/home/" in serialized or "registry+https://" in serialized:
        raise ValueError("refusing an SBOM containing source-machine paths")
    output.parent.mkdir(parents=True, exist_ok=True); output.write_text(serialized)
    inventory = {"format": "siliq-third-party-notices", "schemaVersion": "1.0.0", "target": target, "sourceLockSha256": lock_hash, "missingLicenseTexts": missing, "packages": sorted(notices, key=lambda p: (p["ecosystem"], p["name"], p["version"]))}
    (license_dir.parent / "license-inventory.json").write_text(json.dumps(inventory, ensure_ascii=False, indent=2) + "\n")
    lines = ["SILIQ Creative Generator — third-party notices", "", "First-party source remains UNLICENSED. This notice does not change its license.", "", "The following dependency versions and upstream license declarations come from", "the locked target-specific normal Cargo graph and installed frontend runtime graph.", "Complete available license texts accompany this notice in licenses/.", "", "Native Skia: tag m153-0.101.2, rust-skia release " + native["release"], "Native archive SHA-256: " + native["sha256"], "Skia license: licenses/native/Skia-BSD-3-Clause.txt", "Public-safe Noto Sans JP fixture fonts: public-safe/licenses/NotoSansJP-OFL-1.1.txt", "", "This software is based in part on the work of the Independent JPEG Group.", "Native vendored licenses: licenses/native-vendored/ (Expat, HarfBuzz, ICU,", "libjpeg-turbo, libpng, Wuffs, zlib), with exact source URLs/revisions and hashes", "in native-license-sources.json. Those revisions are Skia DEPS declarations,", "not an attestation of the contents of the prebuilt binary archive.", "", "selectors 0.38.0 (MPL-2.0) corresponding source is included unchanged at", "sources/selectors-0.38.0.crate; its SHA-256 is documented in sources/NOTICE.", "Additional upstream license snapshots and supplemental objc2 full texts", "are documented in upstream-license-sources.json. No license is assigned to", "third-party code beyond its original upstream declaration.", "", "Operating-system frameworks are prerequisites, not redistributed by this app.", "Native vendored library provenance/CVE review is tracked separately; the SBOM", "does not claim to prove the complete C/C++ contents of a prebuilt archive.", ""]
    lines += [f"{p['ecosystem']}: {p['name']} {p['version']} — {p['license']}" for p in inventory["packages"]]
    if missing:
        lines += ["", "Missing upstream license texts (requires resolution before public distribution):", *missing]
    (license_dir.parent / "NOTICE").write_text("\n".join(lines) + "\n")
    print(f"Generated {len(components)} components, {len(notices)} dependency notices; {len(missing)} license-text gaps.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--package", default="siliq-native-bridge")
    parser.add_argument("--target", default=host_target())
    parser.add_argument("--output", type=pathlib.Path)
    parser.add_argument("--licenses-dir", type=pathlib.Path, default=ROOT / "third_party/licenses")
    args = parser.parse_args()
    output = args.output or ROOT / ("third_party/sbom-" + args.target + ".json")
    generate(args.package, args.target, output.resolve(), args.licenses_dir.resolve())
