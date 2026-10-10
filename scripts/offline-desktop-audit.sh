#!/usr/bin/env bash
# Static release evidence and a target-specific normal-dependency SBOM.
# Actual attributed traffic/airplane-mode acceptance remains a separate test.
set -euo pipefail
audit_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
audit_bundle="${1:-}"
audit_output="${2:-$audit_root/out/desktop-audit}"
audit_target="$(rustc -vV | sed -n 's/^host: //p')"
mkdir -p "$audit_output"
cargo metadata --offline --locked --format-version 1 --filter-platform "$audit_target" --features siliq-desktop/custom-protocol --manifest-path "$audit_root/Cargo.toml" > "$audit_output/cargo-metadata.json"
cargo tree --offline --locked -p siliq-native-bridge --target "$audit_target" --edges normal,features > "$audit_output/normal-features.txt"
python3 - "$audit_root" "$audit_output" "$audit_target" <<'PY'
import json,re,sys
from pathlib import Path
root,output,target=Path(sys.argv[1]),Path(sys.argv[2]),sys.argv[3]
metadata=json.loads((output/'cargo-metadata.json').read_text())
packages={p['id']:p for p in metadata['packages']}
nodes={p['id']:p for p in metadata['resolve']['nodes']}
start=next(p['id'] for p in packages.values() if p['name']=='siliq-native-bridge')
seen=set()
def walk(key):
    if key in seen:return
    seen.add(key)
    for dependency in nodes[key]['deps']:
        if any(kind['kind'] is None for kind in dependency['dep_kinds']):walk(dependency['pkg'])
walk(start)
records=[];failures=[]
normal_features={}
for name,feature in re.findall(r'\b([A-Za-z0-9_-]+) feature "([^"]+)"',(output/'normal-features.txt').read_text()):normal_features.setdefault(name,set()).add(feature)
for key in sorted(seen,key=lambda x:(packages[x]['name'],packages[x]['version'])):
    package=packages[key]
    macro=any('proc-macro' in t['kind'] for t in package['targets'])
    record={k:package.get(k) for k in ['name','version','license','source','repository']}
    # cargo metadata merges build/target feature sets for a package. The
    # filtered normal feature tree separates the release runtime activation.
    record.update(features=sorted(normal_features.get(package['name'],set())),metadataResolvedFeatures=nodes[key]['features'],role='compile-time proc-macro' if macro else 'normal dependency')
    records.append(record)
    if re.match(r'^(reqwest|hyper|hyper-util|ureq|isahc|surf|awc|tungstenite|tokio-tungstenite|async-openai|openai|ort|onnxruntime|tch|candle-core|llama-cpp|tauri-plugin-(http|websocket|updater|shell|opener))$',package['name']):failures.append('forbidden normal dependency: '+package['name'])
    if package['name']=='tokio' and set(record['features'])&{'net','process'}:failures.append('Tokio networking/process feature is enabled')
config=json.loads((root/'apps/desktop/src-tauri/tauri.conf.json').read_text())
capability=json.loads((root/'apps/desktop/src-tauri/capabilities/main.json').read_text())
manifest=(root/'apps/desktop/src-tauri/build.rs').read_text()
commands=set(re.findall(r'"([a-z_]+)"',re.search(r'commands\(&\[(.*?)\]\)',manifest,re.S).group(1)))
permissions={'allow-'+command.replace('_','-') for command in commands}
if set(capability['permissions'])!=permissions:failures.append('native command manifest and capability differ')
if capability.get('remote') or capability.get('windows')!=['main'] or capability.get('local') is not True:failures.append('capability is not limited to the main local window')
csp=config['app']['security']['csp']
if "default-src 'none'" not in csp or 'connect-src ipc: http://ipc.localhost;' not in csp or '*' in csp:failures.append('release CSP is not default-deny with local IPC only')
if config['app'].get('withGlobalTauri'):failures.append('global Tauri API is exposed')
if config['app']['security'].get('dangerousDisableAssetCspModification'):failures.append('automatic asset CSP protection is disabled')
rust_sources=list((root/'apps/desktop/src-tauri/src').glob('*.rs'))+list((root/'crates/native-bridge/src').glob('*.rs'))
for source in rust_sources:
    text=source.read_text()
    if re.search(r'std::process::Command|tokio::process|TcpStream|TcpListener|UdpSocket|reqwest::|WebSocket',text):failures.append('unexpected process/network call in '+source.name)
for source in (root/'apps/desktop/ui').glob('*'):
    if source.suffix in {'.ts','.tsx'} and re.search(r'\bfetch\s*\(|XMLHttpRequest|WebSocket|EventSource|dangerouslySetInnerHTML',source.read_text()):failures.append('unexpected browser network/raw-markup call in '+source.name)
swift_sources=list((root/'apps/macos/Sources').glob('*.swift'))
if not swift_sources:failures.append('native SwiftUI sources are missing')
for source in swift_sources:
    if re.search(r'\bimport\s+(WebKit|Network|FoundationNetworking)\b|\b(URLSession|NSURLConnection|URLRequest|WKWebView|NSTask)\b|\bProcess\s*\(',source.read_text()):failures.append('unexpected webview/network/process call in '+source.name)
broker=(root/'apps/desktop/src-tauri/src/native.rs').read_text()
native_commands=set(re.findall(r'"([a-z_]+)"\s*=>',broker))
native_commands.update({'undo_edit','redo_edit'})
if not {'native_preview','open_project_path','export_design_path','preflight_batch_path'}<=native_commands:failures.append('native broker command allowlist is incomplete')
ffi=(root/'crates/native-bridge/src/lib.rs').read_text()
for name in ['create','request','last_error','free','close']:
    if 'fn siliq_native_'+name not in ffi:failures.append('missing C ABI operation: '+name)
sbom={'target':target,'rootPackage':'siliq-native-bridge','scope':'native broker target-specific normal dependency graph; proc-macros explicitly marked; build/dev dependency edges excluded; Swift/OS frameworks tracked separately','packages':records}
(output/'runtime-sbom.json').write_text(json.dumps(sbom,ensure_ascii=False,indent=2)+'\n')
result={'target':target,'rootPackage':'siliq-native-bridge','normalPackageCount':len(records),'staticChecksPassed':not failures,'failures':failures,'nativeCommandCount':len(native_commands),'swiftSourceCount':len(swift_sources),'legacyTauriReference':{'capabilityCommandCount':len(commands),'localCspAndCapabilityVerified':True,'productWebViewInitialized':False},'tokioRationale':'Retained Tauri host code requires Tokio; the SwiftUI broker calls synchronous plain helpers. net/process features are rejected.','tauriFsRationale':'tauri-plugin-fs remains a retained dialog FilePath dependency; the native broker initializes no plugin or WebView.','runtimeTrafficObserved':False,'scopeLimit':'Static checks, symbols, and entitlements do not prove absence of attributed connection attempts or an offline first-launch workflow.'}
(output/'static-audit.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
if failures:raise SystemExit('\n'.join(failures))
print(f'Static native audit passed: {len(records)} normal packages; {len(native_commands)} broker commands; {len(swift_sources)} Swift sources. Legacy Tauri checks are reference evidence.')
PY
if [[ -n "$audit_bundle" ]]; then
    [[ -d "$audit_bundle/Contents" ]] || { echo "Expected a macOS .app bundle" >&2; exit 1; }
    codesign --verify --deep --strict "$audit_bundle"
    codesign -d --entitlements :- "$audit_bundle" > "$audit_output/entitlements.plist" 2> "$audit_output/codesign-info.txt"
    python3 - "$audit_bundle" "$audit_output" <<'PY'
import json,plistlib,subprocess,sys,hashlib,re
from pathlib import Path
bundle,output=Path(sys.argv[1]),Path(sys.argv[2])
entitlements=plistlib.loads((output/'entitlements.plist').read_bytes())
assert entitlements.get('com.apple.security.app-sandbox') is True,'App Sandbox is missing'
assert not any('network' in k for k in entitlements),'A network entitlement is present'
assert not any('temporary-exception' in k for k in entitlements),'A broad sandbox exception is present'
assert entitlements.get('com.apple.security.files.user-selected.read-write') is True,'User-selected file grant entitlement is missing'
info=plistlib.loads((bundle/'Contents/Info.plist').read_bytes())
binary=bundle/'Contents/MacOS'/info['CFBundleExecutable']
native=info['CFBundleExecutable']=='siliq-studio'
imports=subprocess.check_output(['nm','-u',str(binary)],text=True)
(output/'native-imports.txt').write_text(imports)
network_pattern=r'\b_(socket|connect|bind|listen|accept|getaddrinfo|sendto|recvfrom|res_query)\b'
network_symbols=[line for line in imports.splitlines() if re.search(network_pattern,line)]
library_result=None
if native:
    executable_links=subprocess.check_output(['otool','-L',str(binary)],text=True)
    (output/'executable-linked-libraries.txt').write_text(executable_links)
    assert '@rpath/libsiliq_native_bridge.dylib' in executable_links,'native executable does not load its bundled broker through rpath'
    assert not re.search(r'^\s+/(?!System/Library/|usr/lib/)',executable_links,re.M),'native executable retains a non-system absolute library path'
    assert not network_symbols,'Swift executable imports direct network symbols'
    assert not re.search(r'WKWebView|URLSession|NSURLConnection',imports),'Swift executable references WebView/network classes'
    library=bundle/'Contents/Frameworks/libsiliq_native_bridge.dylib'
    assert library.is_file(),'native broker dylib is missing'
    subprocess.run(['codesign','--verify','--strict',str(library)],check=True)
    lib_ent=subprocess.run(['codesign','-d','--entitlements',':-',str(library)],capture_output=True,check=True)
    library_entitlements=plistlib.loads(lib_ent.stdout) if lib_ent.stdout.strip() else {}
    assert not any('network' in k or 'temporary-exception' in k for k in library_entitlements),'dylib contains network/exception entitlement'
    lib_imports=subprocess.check_output(['nm','-u',str(library)],text=True)
    (output/'broker-imports.txt').write_text(lib_imports)
    lib_network=[line for line in lib_imports.splitlines() if re.search(network_pattern,line)]
    assert not lib_network,'native broker imports direct network symbols'
    assert not re.search(r'WKWebView|URLSession|NSURLConnection',lib_imports),'native broker references WebView/network classes'
    linked=subprocess.check_output(['otool','-L',str(library)],text=True)
    (output/'broker-linked-libraries.txt').write_text(linked)
    library_result={'signatureVerified':True,'sha256':hashlib.sha256(library.read_bytes()).hexdigest(),'entitlements':library_entitlements,'directNetworkSymbolImports':lib_network,'webKitFrameworkRetainedLink':'WebKit.framework' in linked,'webKitLinkRationale':'The shared host retains Tauri dependency link declarations; SwiftUI initializes no WebView, and direct WKWebView class references are rejected. Actual child-process observation remains separate.'}
result={'interface':'native-swiftui' if native else 'legacy-tauri-reference','codeSignatureVerified':True,'appSandbox':True,'networkEntitlements':False,'entitlements':entitlements,'executableSha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'architectures':subprocess.check_output(['lipo','-archs',str(binary)],text=True).strip(),'directNetworkSymbolImports':network_symbols,'nativeBroker':library_result,'runtimeTrafficObserved':False,'distributionNotarizationVerified':False,'scopeLimit':'Imported symbols are recorded, not treated as traffic evidence. This audit does not claim notarization or network-isolated first-launch acceptance.'}
(output/'bundle-audit.json').write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
print('Bundle signature and Sandbox entitlements verified; no network entitlement. Runtime observation is a separate acceptance gate.')
PY
fi
