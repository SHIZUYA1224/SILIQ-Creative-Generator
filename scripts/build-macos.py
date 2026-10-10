#!/usr/bin/env python3
"""Build the network-denied SwiftUI app with the shared verified Rust engine."""
import argparse
import json
import pathlib
import plistlib
import shutil
import subprocess

ROOT = pathlib.Path(__file__).resolve().parents[1]

def run(*args):
    subprocess.run(args, cwd=ROOT, check=True)

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--debug', action='store_true')
    parser.add_argument('--no-sign', action='store_true')
    parser.add_argument('--signing-identity', default='-')
    args = parser.parse_args()
    if subprocess.check_output(['uname', '-m'], text=True).strip() != 'arm64':
        raise SystemExit('The verified native app currently targets Apple Silicon.')
    # Keep the retained Tauri editor reproducible: its Rust context embeds this
    # build asset even though the SwiftUI application initializes no WebView.
    run('npm', 'run', 'build')
    cargo_args = ['bash', str(ROOT/'scripts/cargo-verified.sh'), 'build', '-p', 'siliq-native-bridge', '--locked']
    if not args.debug:
        cargo_args.append('--release')
    run(*cargo_args)
    run('python3', str(ROOT/'scripts/generate-sbom.py'), '--package', 'siliq-native-bridge')
    profile = 'debug' if args.debug else 'release'
    target = ROOT/'target'/profile
    bundle = target/'bundle/macos/SILIQ Creative Generator.app'
    if bundle.exists():
        # Only this task's generated app is replaced, never projects or outputs.
        shutil.rmtree(bundle)
    contents = bundle/'Contents'
    for name in ['MacOS', 'Frameworks', 'Resources']:
        (contents/name).mkdir(parents=True)
    dylib = contents/'Frameworks/libsiliq_native_bridge.dylib'
    shutil.copy2(target/'libsiliq_native_bridge.dylib', dylib)
    run('install_name_tool', '-id', '@rpath/libsiliq_native_bridge.dylib', str(dylib))
    sources = sorted((ROOT/'apps/macos/Sources').glob('*.swift'))
    executable = contents/'MacOS/siliq-studio'
    run('xcrun', 'swiftc', '-swift-version', '5', '-target', 'arm64-apple-macos13.0',
        '-O' if not args.debug else '-Onone', '-import-objc-header',
        str(ROOT/'crates/native-bridge/include/siliq_native.h'),
        *map(str, sources), '-L', str(contents/'Frameworks'), '-lsiliq_native_bridge',
        '-Xlinker', '-rpath', '-Xlinker', '@executable_path/../Frameworks', '-o', str(executable))
    linked = subprocess.check_output(['otool', '-L', str(executable)], text=True)
    if '@rpath/libsiliq_native_bridge.dylib' not in linked or any(str(ROOT) in line for line in linked.splitlines()[1:]):
        raise RuntimeError('The app must load its packaged broker, without a checkout dependency.')
    resources = contents/'Resources'
    shutil.copytree(ROOT/'fixtures/public-safe', resources/'public-safe')
    shutil.copy2(ROOT/'apps/desktop/src-tauri/icons/icon.icns', resources/'icon.icns')
    for name in ['licenses', 'sources']:
        shutil.copytree(ROOT/'third_party'/name, resources/name)
    for source, dest in [('NOTICE','NOTICE'),('sbom-aarch64-apple-darwin.json','sbom.json'),
                         ('license-inventory.json','license-inventory.json'),
                         ('upstream-license-sources.json','upstream-license-sources.json'),
                         ('native-license-sources.json','native-license-sources.json')]:
        shutil.copy2(ROOT/'third_party'/source, resources/dest)
    info = {'CFBundleIdentifier':'jp.siliq.creative-generator','CFBundleName':'SILIQ Creative Generator',
            'CFBundleDisplayName':'SILIQ Creative Generator','CFBundleExecutable':'siliq-studio',
            'CFBundlePackageType':'APPL','CFBundleShortVersionString':'0.1.0','CFBundleVersion':'1',
            'CFBundleIconFile':'icon.icns','LSMinimumSystemVersion':'13.0',
            'NSPrincipalClass':'NSApplication','NSHighResolutionCapable':True,
            'CFBundleDocumentTypes':[{'CFBundleTypeName':'SILIQ project','CFBundleTypeRole':'Editor',
              'LSHandlerRank':'Owner','CFBundleTypeExtensions':['siliqcg']} ]}
    (contents/'Info.plist').write_bytes(plistlib.dumps(info))
    if not args.no_sign:
        run('codesign','--force','--sign',args.signing_identity,str(dylib))
        run('codesign','--force','--sign',args.signing_identity,'--entitlements',
            str(ROOT/'apps/desktop/src-tauri/Entitlements.plist'),str(bundle))
        run('codesign','--verify','--deep','--strict',str(bundle))
    print(f'Built native SwiftUI app: {bundle}')

if __name__ == '__main__':
    main()
