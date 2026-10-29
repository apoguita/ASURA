#!/usr/bin/env python3
"""Point the client's baked-in ArenaNet hostnames at our server, or put them back.

This is the companion of `patch-gw.py --detach`. The patcher rewrites the hostnames inside the
executable, but the client also resolves them at runtime, so the machine's hosts file is what makes
*our* service the one that answers for names like `Auth1.ArenaNetworks.com` and the file servers.

Default state is OFF: the client resolves those names normally and connects live, which is a working
configuration. Turning it on is a deliberate, machine-wide test setting: while the block is present,
ANY client on this machine that resolves these names is redirected to the target address, including a
retail game. That is why enable/disable/status exist and why every other line of the file is left
byte for byte alone.

  python client-patcher/hosts-redirect.py status     what is on now (default)
  python client-patcher/hosts-redirect.py enable     add the block (elevated shell on Windows)
  python client-patcher/hosts-redirect.py disable    remove the block, everything else untouched

Options: --hosts PATH (default: the operating system's hosts file), --target ADDRESS (default
127.0.0.1 — an address our server answers on).

Ported from GWPS `tools/hosts-gwps.ps1` (same block, same reversibility), as Python so it is the same
command on Windows and Linux.
"""

import argparse
import os
import re
import shutil
import sys
import time

BEGIN = '# --- ASURA private server ---'
END = '# --- end ASURA ---'

# The names the client is known to look up. The File* family is covered as a small range rather than
# one-by-one: the client was observed using more than one file server, and a name that does not exist
# is simply never queried -- while a missing entry for one it does use is a live socket.
HOSTNAMES = (
    'Auth1.ArenaNetworks.com',
    'File1.ArenaNetworks.com',
    'File2.ArenaNetworks.com',
    'File3.ArenaNetworks.com',
    'File4.ArenaNetworks.com',
    'File5.ArenaNetworks.com',
    'File6.ArenaNetworks.com',
    'Diag.ArenaNetworks.com',
    'icmp.arenanetworks.com',
    'bug.arenanetworks.com',
    'account.arena.net',
)


def default_hosts_path() -> str:
    if os.name == 'nt':
        root = os.environ.get('SystemRoot', r'C:\Windows')
        return os.path.join(root, 'System32', 'drivers', 'etc', 'hosts')
    return '/etc/hosts'


def read(path: str) -> str:
    with open(path, 'rb') as f:
        return f.read().decode('utf-8', errors='surrogateescape')


def write(path: str, content: str) -> None:
    # The hosts file is machine-wide and may be ANSI; keep the bytes otherwise as they were.
    with open(path, 'wb') as f:
        f.write(content.encode('utf-8', errors='surrogateescape'))


def enabled(content: str) -> bool:
    return any(line.strip() == BEGIN for line in content.splitlines())


def block(target: str) -> str:
    lines = [BEGIN, '# client-patcher/hosts-redirect.py - remove with the disable action']
    lines += [f'{target} {name}' for name in HOSTNAMES]
    lines.append(END)
    return '\r\n'.join(lines)


def status(path: str) -> None:
    content = read(path)
    if enabled(content):
        print('enabled: those hostnames resolve to this machine (retail clients are redirected too)')
    else:
        print('disabled (default): those hostnames resolve normally and the client connects live')


def enable(path: str, target: str) -> None:
    content = read(path)
    if enabled(content):
        print('already enabled')
        return
    backup = path + '.asura-backup'
    if not os.path.exists(backup):
        shutil.copyfile(path, backup)
        print(f'backed up the original to {backup}')
    newline = '\r\n' if '\r\n' in content else '\n'
    if not content.endswith(newline):
        content += newline
    write(path, content + newline + block(target) + newline)
    print(f'enabled: {len(HOSTNAMES)} hostname(s) resolve to {target} (block added to {path})')


def disable(path: str) -> None:
    content = read(path)
    if not enabled(content):
        print('already disabled')
        return
    kept = []
    skipping = False
    for line in content.splitlines(keepends=True):
        stripped = line.strip()
        if stripped == BEGIN:
            skipping = True
            continue
        if stripped == END:
            skipping = False
            continue
        if not skipping:
            kept.append(line)
    write(path, ''.join(kept))
    print(f'disabled: the block was removed from {path}; every other entry is untouched')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument('action', choices=['status', 'enable', 'disable'])
    parser.add_argument('--hosts', default=default_hosts_path(),
                        help='the hosts file to edit (default: the operating system\'s)')
    parser.add_argument('--target', default='127.0.0.1',
                        help='the address the hostnames resolve to (default 127.0.0.1)')
    args = parser.parse_args()

    if not os.path.exists(args.hosts):
        print(f'no hosts file at {args.hosts}', file=sys.stderr)
        return 2

    try:
        if args.action == 'status':
            status(args.hosts)
        elif args.action == 'enable':
            enable(args.hosts, args.target)
        else:
            disable(args.hosts)
    except PermissionError:
        print(f'cannot write {args.hosts}: run this from an elevated shell (on Windows, the hosts '
              f'file needs administrator rights)', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
