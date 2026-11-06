#!/usr/bin/env python3
"""Publishes the client that tools/native/vps_build.sh built on a Linux box as a GitHub release.

Usage: vps_release.py <ssh-target> [--key FILE] [--work DIR] [--repo OWNER/NAME] [--backup DIR]

What it does, in order:
  1. copies dist/ (the exe, SHA256SUMS, compat.json, tag) from the build box;
  2. checks the exe against SHA256SUMS;
  3. saves the assets of the release that has this tag now (if any) in --backup, so it can be put back;
  4. deletes that release and its tag, like the workflow does, and creates the release again, published
     (not a draft, not a pre-release) with the three assets.

The launcher offers a release named like the Nightly its engine came from as a client update, so this
reaches every player on that Nightly. GitHub access is the credential git already holds for github.com
(`git credential fill`); it is passed to GitHub only and never printed.
"""
import argparse
import hashlib
import json
import pathlib
import subprocess
import sys
import tempfile
import urllib.error
import urllib.request

ASSET = 'horizon-client-windows-x64.exe'
FILES = [ASSET, 'SHA256SUMS', 'compat.json']


def token():
    out = subprocess.run(['git', 'credential', 'fill'], input='protocol=https\nhost=github.com\n\n',
                         capture_output=True, text=True, check=True).stdout
    for line in out.splitlines():
        if line.startswith('password='):
            return line.split('=', 1)[1]
    raise SystemExit('vps_release: git holds no GitHub credential. Sign in with git first.')


def call(method, url, secret, body=None, content_type='application/json', raw=False):
    data = body if isinstance(body, (bytes, type(None))) else json.dumps(body).encode()
    request = urllib.request.Request(url, data=data, method=method, headers={
        'Authorization': f'Bearer {secret}', 'Accept': 'application/vnd.github+json',
        'User-Agent': 'native-vps-release', 'Content-Type': content_type})
    try:
        with urllib.request.urlopen(request, timeout=300) as response:
            payload = response.read()
            return payload if raw else (json.loads(payload) if payload else None)
    except urllib.error.HTTPError as error:
        if error.code == 404 and method == 'GET':
            return None
        raise SystemExit(f'vps_release: {method} {url} -> HTTP {error.code}: {error.read()[:300]!r}')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('target')
    parser.add_argument('--key')
    parser.add_argument('--work', default='native-rust')
    parser.add_argument('--repo', default='atlas-thedev/horizon')
    parser.add_argument('--backup', default='release-backup')
    args = parser.parse_args()

    dist = pathlib.Path(tempfile.mkdtemp(prefix='native-dist-'))
    ssh = ['-o', 'BatchMode=yes'] + (['-i', args.key] if args.key else [])
    for name in FILES + ['tag']:
        subprocess.run(['scp', '-q', *ssh, f'{args.target}:{args.work}/dist/{name}', str(dist / name)], check=True)
    tag = (dist / 'tag').read_text().strip()
    compat = json.loads((dist / 'compat.json').read_text())
    digest = hashlib.sha256((dist / ASSET).read_bytes()).hexdigest()
    listed = {l.split()[1].lstrip('*'): l.split()[0] for l in (dist / 'SHA256SUMS').read_text().splitlines() if l.strip()}
    if listed.get(ASSET) != digest:
        raise SystemExit('vps_release: the exe does not match SHA256SUMS.')
    print(f'{tag}: {ASSET} {digest[:12]}… ({(dist / ASSET).stat().st_size >> 20} MiB)')

    secret = token()
    api = f'https://api.github.com/repos/{args.repo}'
    old = call('GET', f'{api}/releases/tags/{tag}', secret)
    if old:
        saved = pathlib.Path(args.backup) / tag
        saved.mkdir(parents=True, exist_ok=True)
        for asset in old['assets']:
            # Public downloads: the launcher fetches the same URLs without signing in.
            download = urllib.request.Request(asset['browser_download_url'], headers={'User-Agent': 'native-vps-release'})
            (saved / asset['name']).write_bytes(urllib.request.urlopen(download, timeout=300).read())
        print(f'saved the release that was live in {saved}')
        call('DELETE', f'{api}/releases/{old["id"]}', secret)
        call('DELETE', f'{api}/git/refs/tags/{tag}', secret)
    release = call('POST', f'{api}/releases', secret, {
        'tag_name': tag, 'name': f'Native engine for {compat["upstream_release"]}',
        'body': f"Rebranded Horizon client built from upstream's {compat['upstream_release']} (MIT, see NATIVE.md).",
        'draft': False, 'prerelease': False})
    upload = release['upload_url'].split('{')[0]
    for name in FILES:
        call('POST', f'{upload}?name={name}', secret, (dist / name).read_bytes(), 'application/octet-stream')
        print(f'uploaded {name}')
    print(release['html_url'])


if __name__ == '__main__':
    sys.exit(main())
