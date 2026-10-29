# The client patcher

Nothing in this project runs without a patched Guild Wars client. The stock `Gw.exe` carries
ArenaNet's Diffie–Hellman parameters, a game-window mutex and ArenaNet hostnames; this script
rewrites them so the client can talk to a server we control. It is the first step of every
client-facing test, and it lives here as first-class tooling, not as a buried utility.

- **`patch-gw.py`** — the patcher. Reads a keys file, writes `Gw.custom.exe`.
- **`hosts-redirect.py`** — the optional companion: puts the client's ArenaNet hostnames into the
  machine's hosts file (or takes them back out), so the patch and the runtime lookups agree.
- **`keys.txt`** — the DH parameters that go into the client. **They are the server's parameters**:
  `prime` and `server_private` are the two constants in
  `crates/asura-protocol/src/client/handshake.rs` (`PRIME_MODULUS_DECIMAL`,
  `SERVER_PRIVATE_DECIMAL`), so a client patched with this file and this server agree on the
  handshake. If the server's constants ever change, this file changes with them — they are one pair.

## What the patch does

| patch | why |
|---|---|
| Diffie–Hellman constants in `.text`/`.rdata` | the client and the server must run the same key exchange; the client carries a copy of the parameters |
| `CreateMutexA` code and mutex name (`AN-Mutex-Window` → `AN-Futex`) | without it the client refuses to start when another copy is running, which kills every two-client test |
| all seven baked-in ArenaNet hostnames, except one | **required, on by default**: the client carries `Auth1`, `File1`, `Diag`, `bug`, `icmp`, `ArenaNetworks.com` and `account.arena.net`, and the patcher repoints them so they stop pointing at ArenaNet. The one exception is `File1.ArenaNetworks.com`, the content/file server — the **optional address**: it stays live unless `--file-server` is passed, and that choice pairs with the hosts redirect below |
| optional (`--file-server`): `File1.ArenaNetworks.com` | **off by default** — the client connects to the real file server and streams `Gw.dat` content from it, which is a working build. Turned on *and* paired with the hosts redirect, that address resolves to this machine and **our service answers it** (the auth service answers the file check) |

What it does **not** touch: the auth address — the client takes that from its command line
(`Gw.custom.exe -authsrv 127.0.0.1 -portal 127.0.0.1`). It does not decompile anything; it finds byte
signatures and overwrites the bytes in place.

## Requirements

- **Windows** — the target is a Windows executable (`Gw.exe`); the script patches a PE image.
- **Python 3** and **`pefile`**: `pip install pefile`.
- The game's own **`Gw.exe`** (the Client archive is not in this repository; it is the same file the
  game installs).

## Build the patched client

```powershell
pip install pefile
python client-patcher/patch-gw.py "F:\GW\GW1\Gw.exe" client-patcher/keys.txt --output "F:\GW\GW1\Gw.custom.exe"
```

Then stage the **portal DLL** next to it (`portal.dll`, next to `Gw.custom.exe`). That DLL is the
client side of the portal login: it asks the web gate on `:6601` who the player is before the auth
connection sends `AUTH_CMSG_PORTAL_ACCOUNT_LOGIN`. Its source is the GWPS repository's
`portal/portal.c` (built with the client rig's toolchain); this project consumes it, it does not
rebuild it. Without it the client opens the auth connection, does the key exchange, sends its
preamble and closes — which looks exactly like "cannot connect" while the server is running.

## Launch

```powershell
F:\GW\GW1\Gw.custom.exe -authsrv 127.0.0.1 -portal 127.0.0.1
```

Log in as one of the seeded accounts (`user1@example.com`, `user2@example.com`,
`user3@example.com`; the web gate does not check the password — `docs/19-running.md`).

Success is the client reaching the character list and entering an outpost. If it fails, the verdict
is the auth service's trace: set `ASURA_TRACE=1` before starting the stack and read the log
(`docs/HANDOFF.md`, the real client is the test).

## Optional: the file-server address

The patcher repoints every baked-in hostname by default, with one exception: `File1.ArenaNetworks.com`,
the content/file server. That address is the optional setting — the client streams `Gw.dat` content
from it, so the default build leaves it live and connects to the real server. Turning it on is a pair:

1. Patch with `--file-server` (this repoints File1 too):
   ```powershell
   python client-patcher/patch-gw.py "F:\GW\GW1\Gw.exe" client-patcher/keys.txt --output "F:\GW\GW1\Gw.custom.exe" --file-server
   ```
2. Redirect that address, from an elevated shell (the hosts file needs administrator rights on
   Windows):
   ```powershell
   python client-patcher/hosts-redirect.py enable --file-server
   ```

With both on, `File1.ArenaNetworks.com` resolves to this machine and **our service answers it** — the
auth service answers the file check, so nothing leaves the machine for that address. The hosts block
follows the patch exactly: `enable` without `--file-server` redirects the six required names and
leaves File1 live, matching a default build.

The hosts block is machine-wide: while it is in, *any* client on this machine resolving those names is
redirected, a retail one included. That is why it is a switch, not a permanent edit — and it is
reversible by design:

```powershell
python client-patcher/hosts-redirect.py status     # what is on now, name by name
python client-patcher/hosts-redirect.py disable    # remove exactly that block, everything else untouched
```

The first `enable` copies the original hosts file to `<hosts>.asura-backup`; `disable` removes only
the marked block and leaves every other entry byte for byte alone. Off (the default), every name on the
list resolves the normal way — File1 included — and the client connects live; that configuration
works, and the switch exists to take the file-server address off ArenaNet when you want that.

## Provenance

`patch-gw.py` is the patcher from the GWPS project (`tools/patch-gw.py`), and `keys.txt` is that
project's keys file, whose parameters this server copied (`docs/04-from-gwps.md`). Both are kept
here verbatim so a clean checkout can produce the client the server expects; when the reference
disappears from disk, this folder is the copy that matters.
