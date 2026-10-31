"""Collect target-specific license texts before bundling. Fail closed on missing texts."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[1]

def metadata(manifest, target):
    return json.loads(subprocess.check_output([
        'cargo', 'metadata', '--locked', '--format-version', '1',
        '--filter-platform', target, '--manifest-path', str(manifest)
    ], cwd=ROOT))['packages']

def license_files(base):
    for item in base.rglob('*'):
        if item.is_file() and (item.name.lower().startswith(('license', 'licence', 'copying', 'copyright')) or 'notice' in item.name.lower()):
            yield item

def fetch_exact_license(package, base, dest):
    vcs_path = base / '.cargo_vcs_info.json'
    if not vcs_path.exists():
        return False
    vcs = json.loads(vcs_path.read_text(encoding='utf-8'))
    repo = (package.get('repository') or '').removesuffix('.git').rstrip('/')
    if not repo.startswith('https://github.com/'):
        return False
    sha = vcs.get('git', {}).get('sha1')
    if not sha:
        return False
    subdir = vcs.get('path_in_vcs', '')
    prefixes = list(dict.fromkeys([subdir, *[str(p) for p in Path(subdir).parents], '']))
    for prefix in prefixes:
        prefix = '' if prefix == '.' else prefix.strip('/')
        for name in ['LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE.txt', 'LICENSE.md']:
            suffix = '/'.join(filter(None, [prefix, name]))
            url = repo.replace('https://github.com/', 'https://raw.githubusercontent.com/') + f'/{sha}/{suffix}'
            try:
                with urllib.request.urlopen(url, timeout=15) as response:
                    data = response.read(1024 * 1024)
                dest.mkdir(parents=True, exist_ok=True)
                (dest / name).write_bytes(data)
                (dest / 'SOURCE.txt').write_text(url + '\n', encoding='utf-8')
                return True
            except urllib.error.HTTPError as error:
                if error.code != 404:
                    raise
    return False

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--target', required=True)
    args = parser.parse_args()
    packages = {}
    for manifest in [ROOT / 'Cargo.toml', ROOT / 'src-tauri/Cargo.toml']:
        for package in metadata(manifest, args.target):
            if package.get('source'):
                packages[package['id']] = package
    rows = []
    missing = []
    for package in packages.values():
        base = Path(package['manifest_path']).parent
        dest = ROOT / 'third-party-licenses' / f"rust-{package['name']}-{package['version']}"
        for source in license_files(base):
            target = dest / source.relative_to(base)
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
        if not dest.exists() or not any(license_files(dest)):
            if not fetch_exact_license(package, base, dest):
                missing.append(package['name'])
        if 'MPL' in (package.get('license') or ''):
            shutil.copytree(base, ROOT / 'third-party-sources' / dest.name, dirs_exist_ok=True)
        rows.append(('Rust', package['name'], package['version'], package.get('license') or 'See texts', dest.relative_to(ROOT).as_posix()))
    lock = json.loads((ROOT / 'package-lock.json').read_text(encoding='utf-8'))
    for relative, package in lock['packages'].items():
        base = ROOT / relative
        if not relative or package.get('dev') or not (base / 'package.json').is_file():
            continue
        info = json.loads((base / 'package.json').read_text(encoding='utf-8'))
        dest = ROOT / 'third-party-licenses' / ('npm-' + info['name'].replace('@', '').replace('/', '_') + '-' + info['version'])
        for source in base.iterdir():
            if source.is_file() and (source.name.lower().startswith(('license', 'licence', 'copying', 'copyright')) or 'notice' in source.name.lower()):
                dest.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, dest / source.name)
        if not dest.exists() or not any(license_files(dest)):
            missing.append(info['name'])
        rows.append(('npm', info['name'], info['version'], str(info.get('license', 'See texts')), dest.relative_to(ROOT).as_posix()))
    if missing:
        raise SystemExit('Missing original license texts: ' + ', '.join(sorted(missing)))
    text = '# Third-party notices\n\n'
    text += f'Target: `{args.target}`. Includes target Cargo graphs and installed production npm packages.\n\n'
    text += 'Components retain their original licenses. CodeTrail does not relicense them. Original texts are in third-party-licenses/; MPL component sources are in third-party-sources/, both included in the installer and tools archive. For alternative licenses, use a supplied permitted license branch.\n\n'
    text += '| Ecosystem | Component | Version | Declared license | Texts |\n| --- | --- | --- | --- | --- |\n'
    for kind, name, version, license_name, path in sorted(rows):
        license_name = license_name.replace('|', ' OR ')
        text += f'| {kind} | {name} | {version} | {license_name} | [{path}]({path}) |\n'
    (ROOT / 'THIRD_PARTY_NOTICES.md').write_text(text, encoding='utf-8')
    print(f'Collected {len(rows)} component notices for {args.target}')

if __name__ == '__main__':
    main()
