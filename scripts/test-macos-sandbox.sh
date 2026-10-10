#!/usr/bin/env bash
# Exercise the packaged broker with the real signed, network-denied Sandbox.
set -euo pipefail
cd "$(dirname "$0")/.."
test "$(uname -s)" = Darwin
test "$(uname -m)" = arm64
python3 - <<'PY'
import hashlib
import json
import pathlib
import plistlib
import re
import shutil
import subprocess
import sys

root = pathlib.Path.cwd()
evidence = root / 'out/sandbox-probe'
bundle = evidence / 'SILIQ Sandbox Probe.app'
source_app = root / 'target/release/bundle/macos/SILIQ Creative Generator.app'
source_library = source_app / 'Contents/Frameworks/libsiliq_native_bridge.dylib'
source = root / 'tests/native-macos/SandboxProbe.swift'
if not source_library.is_file():
    raise SystemExit('Build the native release app before running its Sandbox probe.')
text = source.read_text()
imports = re.findall(r'^import\s+(\w+)', text, re.M)
if imports != ['Foundation'] or re.search(r'\b(?:Process|NSTask|URLSession|NSURLConnection|URLRequest|WKWebView)\b', text):
    raise SystemExit('The probe must use Foundation and the C ABI without UI, process or network APIs.')
evidence.mkdir(parents=True, exist_ok=True)
for name in ['result.json', 'signature-audit.json', 'signature.txt', 'entitlements.plist', 'probe.log', 'probe.stderr.log']:
    (evidence / name).unlink(missing_ok=True)
if bundle.exists():
    # Only replace this repository's generated probe bundle, never container data.
    shutil.rmtree(bundle)
contents = bundle / 'Contents'
for name in ['MacOS', 'Frameworks', 'Resources']:
    (contents / name).mkdir(parents=True)
library = contents / 'Frameworks/libsiliq_native_bridge.dylib'
shutil.copy2(source_library, library)
source_sha = hashlib.sha256(source_library.read_bytes()).hexdigest()
assert hashlib.sha256(library.read_bytes()).hexdigest() == source_sha
shutil.copytree(source_app / 'Contents/Resources/public-safe', contents / 'Resources/public-safe')
info = {
    'CFBundleIdentifier': 'jp.siliq.creative-generator.sandbox-tests',
    'CFBundleExecutable': 'siliq-sandbox-probe',
    'CFBundleName': 'SILIQ Sandbox Probe',
    'CFBundlePackageType': 'APPL',
    'CFBundleShortVersionString': '0.1.0',
    'CFBundleVersion': '1',
    'LSMinimumSystemVersion': '13.0',
    'LSUIElement': True,
}
(contents / 'Info.plist').write_bytes(plistlib.dumps(info))
executable = contents / 'MacOS/siliq-sandbox-probe'
subprocess.run([
    'xcrun', 'swiftc', '-parse-as-library', '-swift-version', '5', '-target', 'arm64-apple-macos13.0', '-O',
    '-import-objc-header', str(root / 'crates/native-bridge/include/siliq_native.h'), str(source),
    '-L', str(contents / 'Frameworks'), '-lsiliq_native_bridge',
    '-Xlinker', '-rpath', '-Xlinker', '@executable_path/../Frameworks', '-o', str(executable)
], check=True)
linked = subprocess.check_output(['otool', '-L', str(executable)], text=True)
if '@rpath/libsiliq_native_bridge.dylib' not in linked or any(str(root) in line for line in linked.splitlines()[1:]):
    raise SystemExit('The probe must use its packaged broker through @rpath.')
subprocess.run(['codesign', '--force', '--sign', '-', str(library)], check=True)
subprocess.run(['codesign', '--force', '--sign', '-', '--entitlements',
                str(root / 'apps/desktop/src-tauri/Entitlements.plist'), str(bundle)], check=True)
subprocess.run(['codesign', '--verify', '--deep', '--strict', str(bundle)], check=True)
signature = subprocess.run(['codesign', '-dv', '--verbose=4', str(bundle)], text=True, capture_output=True, check=True)
(evidence / 'signature.txt').write_text(signature.stdout + signature.stderr)
entitlements = subprocess.run(['codesign', '-d', '--entitlements', ':-', str(executable)], capture_output=True, check=True)
(evidence / 'entitlements.plist').write_bytes(entitlements.stdout)
actual = plistlib.loads(entitlements.stdout)
expected = plistlib.loads((root / 'apps/desktop/src-tauri/Entitlements.plist').read_bytes())
if actual != expected or actual != {
    'com.apple.security.app-sandbox': True,
    'com.apple.security.files.user-selected.read-write': True,
}:
    raise SystemExit('Probe signature does not have exactly the product network-denied entitlements.')
library_entitlements = subprocess.run(['codesign', '-d', '--entitlements', ':-', str(library)], capture_output=True, check=True)
if library_entitlements.stdout and plistlib.loads(library_entitlements.stdout):
    raise SystemExit('The packaged broker library must not introduce entitlements.')
audit = {
    'appId': info['CFBundleIdentifier'], 'architecture': 'arm64', 'minimumSystemVersion': '13.0',
    'entitlements': actual, 'signatureVerified': True, 'sourcePackagedBrokerSha256': source_sha,
    'probeBrokerSha256': hashlib.sha256(library.read_bytes()).hexdigest(),
    'probeSourceSha256': hashlib.sha256(source.read_bytes()).hexdigest(),
    'executableSha256': hashlib.sha256(executable.read_bytes()).hexdigest(),
    'runtimeScope': 'unique run in the probe Application Support container; bundled public-safe fixtures only',
}
(evidence / 'signature-audit.json').write_text(json.dumps(audit, indent=2) + '\n')
try:
    completed = subprocess.run([str(executable)], capture_output=True, text=True, timeout=90)
except subprocess.TimeoutExpired as error:
    (evidence / 'probe.log').write_text((error.stdout or b'').decode(errors='replace') if isinstance(error.stdout, bytes) else (error.stdout or ''))
    (evidence / 'probe.stderr.log').write_text((error.stderr or b'').decode(errors='replace') if isinstance(error.stderr, bytes) else (error.stderr or ''))
    raise SystemExit('Signed Sandbox probe exceeded its 90-second bound.')
(evidence / 'probe.log').write_text(completed.stdout)
(evidence / 'probe.stderr.log').write_text(completed.stderr)
try:
    result = json.loads(completed.stdout)
except json.JSONDecodeError:
    sys.stderr.write(completed.stderr)
    raise SystemExit('Signed Sandbox probe did not return structured evidence.')
(evidence / 'result.json').write_text(json.dumps(result, indent=2) + '\n')
print(completed.stdout.strip())
if completed.returncode != 0 or result.get('ok') is not True:
    sys.stderr.write(completed.stderr)
    raise SystemExit(completed.returncode or 1)
print('Signed App Sandbox probe passed; evidence: out/sandbox-probe')
PY
