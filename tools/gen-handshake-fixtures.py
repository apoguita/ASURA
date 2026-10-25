#!/usr/bin/env python3
"""Generate crates/asura-protocol/src/client/handshake_fixtures.rs from the GWPS reference.

The point of this script: the handshake's arithmetic must match the reference *exactly*, and the one
thing a port can get subtly wrong is the key derivation (`arc4_hash`) -- a 40-line custom
construction that is not SHA-1, has no padding and no second block. Reading it carefully is not the
same as proving it.

So this script does not re-implement anything. It compiles a C program out of the reference's own
source text -- `arc4.c` verbatim, and `arc4_hash` extracted verbatim from `AuthSrv.c` -- runs it, and
writes the numbers it prints into a Rust fixture file. The Rust tests then assert that the port
against those numbers.

The modular exponentiation is generated independently, with CPython's arbitrary-precision `pow`,
because that is a different implementation in a different language rather than the same code twice.

Usage:
  python tools/gen-handshake-fixtures.py [--gwps PATH] [--out PATH] [--check]

  --check regenerates and fails if the file on disk differs, which is how a drift is caught.

Needs a C compiler. It looks for cl.exe under Visual Studio (calling vcvars64.bat first) and for gcc
on PATH. With neither, it says so and exits 2 rather than writing fixtures it cannot vouch for.

ASCII-only, like everything else in tools/.
"""

import argparse
import os
import re
import subprocess
import sys
import tempfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_GWPS = os.path.join(os.path.dirname(REPO), "PreSearing")
DEFAULT_OUT = os.path.join(
    REPO, "crates", "asura-protocol", "src", "client", "handshake_fixtures.rs"
)

# The Diffie-Hellman constants are read from the reference's `main.c` rather than retyped here: a
# hand-split decimal string is exactly the kind of silent typo that makes every fixture wrong at once.
# (That happened once already while writing this file, which is why it now parses.)
PRIME_PATTERN = re.compile(r'prime_modulus_ascii\s*=\s*"(\d+)"')
SERVER_PRIVATE_PATTERN = re.compile(r'server_private_ascii\s*=\s*"(\d+)"')

# Fixed inputs, so the fixtures are reproducible and diffable.
MASTER_SECRETS = [
    bytes(range(0, 20)),
    bytes([0x00] * 20),
    bytes([0xFF] * 20),
    bytes.fromhex("5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a"),
]
RC4_KEYS = [
    bytes(range(0, 20)),
    bytes.fromhex("000102030405060708090a0b0c0d0e0f10111213"),
]
PUBLIC_VALUES = [
    # 1, 2, a small odd value, and a value with the top bit set (the largest a 64-byte field carries).
    bytes([1] + [0] * 63),
    bytes([2] + [0] * 63),
    bytes.fromhex("efcdab8967452301") + bytes(56),
    bytes([0xFF] * 64),
]


def read(path):
    with open(path, "r", encoding="utf-8", errors="replace") as handle:
        return handle.read()


def extract_arc4_hash(authsrv_c):
    """Pull `void arc4_hash(...)` out of AuthSrv.c, verbatim, by brace matching."""
    start = authsrv_c.find("void arc4_hash(")
    if start < 0:
        sys.exit("error: arc4_hash not found in AuthSrv.c")
    depth = 0
    for index in range(start, len(authsrv_c)):
        char = authsrv_c[index]
        if char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                return authsrv_c[start : index + 1]
    sys.exit("error: unbalanced braces in arc4_hash")


def c_program(gwps):
    """A C program made of the reference's own text plus the smallest possible shims."""
    open_tyria = os.path.join(gwps, "code", "OpenTyria")
    arc4_c = read(os.path.join(open_tyria, "arc4.c"))
    arc4_hash = extract_arc4_hash(read(os.path.join(open_tyria, "AuthSrv.c")))

    preamble = """
#include <stdio.h>
#include <stdint.h>
#include <string.h>

/* The reference's little-endian helpers (endian.h) and the rotate (macros.h), reproduced because the
 * reference's own headers pull in its whole precompiled-header chain. */
static uint32_t le32dec(const void *pp) {
    const uint8_t *p = (const uint8_t *)pp;
    return (uint32_t)p[0] | ((uint32_t)p[1] << 8) | ((uint32_t)p[2] << 16) | ((uint32_t)p[3] << 24);
}
static void le32enc(void *pp, uint32_t u) {
    uint8_t *p = (uint8_t *)pp;
    p[0] = (uint8_t)(u & 0xff); p[1] = (uint8_t)((u >> 8) & 0xff);
    p[2] = (uint8_t)((u >> 16) & 0xff); p[3] = (uint8_t)((u >> 24) & 0xff);
}
static uint32_t ROL32(uint32_t a, unsigned n) { return (a << n) | (a >> (32 - n)); }

typedef struct arc4_context {
    int x;
    int y;
    uint8_t m[256];
} arc4_context;
"""

    printer = """
static void print_hex(const char *label, const uint8_t *bytes, size_t count) {
    printf("%s ", label);
    for (size_t idx = 0; idx < count; ++idx) printf("%02x", bytes[idx]);
    printf("\\n");
}

int main(void) {
    /* arc4_hash over the fixed master secrets. */
    {
        const uint8_t masters[][20] = {
            {0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19},
            {0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0},
            {255,255,255,255,255,255,255,255,255,255,255,255,255,255,255,255,255,255,255,255},
            {90,90,90,90,90,90,90,90,90,90,90,90,90,90,90,90,90,90,90,90},
        };
        for (size_t idx = 0; idx < sizeof(masters) / sizeof(masters[0]); ++idx) {
            uint8_t digest[20];
            arc4_hash(masters[idx], digest);
            printf("hash %zu ", idx);
            for (size_t byte = 0; byte < sizeof(digest); ++byte) printf("%02x", digest[byte]);
            printf("\\n");
        }
    }

    /* The first 32 keystream bytes for two fixed keys, through the reference's own arc4_crypt. */
    {
        uint8_t key_a[20];
        for (size_t idx = 0; idx < sizeof(key_a); ++idx) key_a[idx] = (uint8_t)idx;
        uint8_t key_b[20] = {0x00,0x01,0x02,0x03,0x04,0x05,0x06,0x07,0x08,0x09,
                             0x0a,0x0b,0x0c,0x0d,0x0e,0x0f,0x10,0x11,0x12,0x13};

        arc4_context ctx;
        uint8_t keystream[32];
        memset(keystream, 0, sizeof(keystream));
        arc4_init(&ctx);
        arc4_setup(&ctx, key_a, sizeof(key_a));
        arc4_crypt(&ctx, keystream, keystream, sizeof(keystream));
        print_hex("arc4 0", keystream, sizeof(keystream));

        memset(keystream, 0, sizeof(keystream));
        arc4_init(&ctx);
        arc4_setup(&ctx, key_b, sizeof(key_b));
        arc4_crypt(&ctx, keystream, keystream, sizeof(keystream));
        print_hex("arc4 1", keystream, sizeof(keystream));
    }

    return 0;
}
"""

    return preamble + "\n/* --- reference arc4.c, verbatim --- */\n" + arc4_c + \
        "\n/* --- reference arc4_hash, extracted verbatim from AuthSrv.c --- */\n" + arc4_hash + \
        "\n" + printer


def find_compiler():
    """Return ("msvc", vcvars_path) or ("gcc", None), or None.

    Both the 32-bit and the 64-bit Program Files are searched: this script may itself be running under
    a 32-bit interpreter, whose `ProgramFiles` points at the x86 tree, and a Build Tools install is as
    good as a full one.
    """
    import glob

    roots = []
    for variable in ("ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"):
        root = os.environ.get(variable)
        if not root:
            continue
        roots.append(os.path.join(root, "Microsoft Visual Studio"))

    for root in roots:
        if not os.path.isdir(root):
            continue
        for vcvars in sorted(
            glob.glob(os.path.join(root, "*", "*", "VC", "Auxiliary", "Build", "vcvars64.bat"))
        ):
            if os.path.isfile(vcvars):
                return ("msvc", vcvars)

    for name in ("gcc", "clang", "cc"):
        found = subprocess.run(
            ["where", name], capture_output=True, text=True, shell=True
        )
        if found.returncode == 0 and found.stdout.strip():
            return ("gcc", None)
    return None


def compile_and_run(source, workdir):
    compiler = find_compiler()
    if compiler is None:
        sys.exit(
            "error: no C compiler found (cl.exe under Visual Studio, or gcc/clang on PATH).\n"
            "The fixtures must come from the reference's own code; they are not guessed at."
        )

    kind, vcvars = compiler
    c_path = os.path.join(workdir, "handshake_fixtures.c")
    exe_path = os.path.join(workdir, "handshake_fixtures.exe")
    with open(c_path, "w", encoding="ascii", newline="\n") as handle:
        handle.write(source)

    if kind == "msvc":
        # A batch file rather than one command line: `cmd /c` mangles the embedded quotes around a
        # path containing spaces ("Program Files"), and the work directory has none.
        script_path = os.path.join(workdir, "build.cmd")
        with open(script_path, "w", encoding="ascii", newline="\r\n") as handle:
            handle.write('@echo off\n')
            handle.write('call "%s" >nul 2>&1\n' % vcvars)
            handle.write('if errorlevel 1 exit /b 1\n')
            handle.write('cl /nologo /W3 /Fe:"%s" "%s" >nul\n' % (exe_path, c_path))
            handle.write('if errorlevel 1 exit /b 1\n')
            handle.write('"%s"\n' % exe_path)
        completed = subprocess.run(
            ["cmd", "/c", script_path], capture_output=True, text=True, cwd=workdir
        )
    else:
        build = subprocess.run(
            ["gcc", "-O1", "-o", exe_path, c_path], capture_output=True, text=True, cwd=workdir
        )
        if build.returncode != 0:
            sys.exit("error: compilation failed:\n" + build.stderr)
        completed = subprocess.run([exe_path], capture_output=True, text=True, cwd=workdir)

    if completed.returncode != 0 or "hash 0" not in completed.stdout:
        sys.exit(
            "error: the reference fixture program did not run (compiler: %s):\n%s%s"
            % (kind, completed.stdout, completed.stderr)
        )
    return completed.stdout


def dh_constants(gwps):
    """The prime modulus and the server private exponent, from the reference's main.c."""
    main_c = read(os.path.join(gwps, "code", "OpenTyria", "main.c"))
    prime = PRIME_PATTERN.search(main_c)
    private = SERVER_PRIVATE_PATTERN.search(main_c)
    if not prime or not private:
        sys.exit("error: the Diffie-Hellman constants are not in main.c where they are expected")
    return int(prime.group(1)), int(private.group(1))


def modpow_fixtures(gwps):
    """(public_le_hex, shared_le_hex) for fixed public values, from CPython's pow()."""
    prime, exponent = dh_constants(gwps)
    rows = []
    for public in PUBLIC_VALUES:
        shared = pow(int.from_bytes(public, "little"), exponent, prime)
        rows.append((public.hex(), shared.to_bytes(64, "little").hex()))
    return rows


def parse_reference_output(text):
    hashes = {}
    arc4_rows = {}
    for line in text.splitlines():
        parts = line.split()
        if len(parts) != 3:
            continue
        if parts[0] == "hash":
            hashes[int(parts[1])] = parts[2]
        elif parts[0] == "arc4":
            arc4_rows[int(parts[1])] = parts[2]
    if len(hashes) != len(MASTER_SECRETS):
        sys.exit("error: expected %d hash fixtures, got %d" % (len(MASTER_SECRETS), len(hashes)))
    if len(arc4_rows) != len(RC4_KEYS):
        sys.exit("error: expected %d arc4 fixtures, got %d" % (len(RC4_KEYS), len(arc4_rows)))
    return hashes, arc4_rows


def bytestring(hex_text):
    return "[" + ", ".join("0x%s" % hex_text[i : i + 2] for i in range(0, len(hex_text), 2)) + "]"


def generate(gwps):
    with tempfile.TemporaryDirectory() as workdir:
        output = compile_and_run(c_program(gwps), workdir)
    hashes, arc4_rows = parse_reference_output(output)
    modpow_rows = modpow_fixtures(gwps)

    lines = []
    add = lines.append
    add("//! Reference outputs for the key exchange, generated from GWPS's own C. DO NOT EDIT BY HAND.")
    add("//!")
    add("//! Generated by `tools/gen-handshake-fixtures.py`. The `arc4` and `arc4_hash` rows come from")
    add("//! compiling the reference's `arc4.c` and its `arc4_hash` (extracted verbatim from")
    add("//! `AuthSrv.c`) and running them; the `modpow` rows come from CPython's `pow()`, which is an")
    add("//! independent implementation of the same arithmetic.")
    add("//!")
    add("//! These exist because the handshake cannot be checked against the game client until the")
    add("//! listener exists, and a wrong key derivation is invisible until the client reports `007`.")
    add("//! `handshake.rs` has tests that assert the port against every number below.")
    add("")
    add("/// `arc4_hash(master)` for a fixed master secret: the ARC4 key the connection is scrambled with.")
    add("///")
    add("/// `#[rustfmt::skip]`: these are one row per case, and rustfmt would rewrap them, which would")
    add("/// make `cargo fmt` and `--check` disagree forever.")
    add("#[rustfmt::skip]")
    add("pub const DERIVED_KEYS: &[([u8; 20], [u8; 20])] = &[")
    for index, _master in enumerate(MASTER_SECRETS):
        add("    (%s, %s)," % (bytestring(master_hex(index)), bytestring(hashes[index])))
    add("];")
    add("")
    add("/// The first 32 bytes of ARC4 keystream for a fixed 20-byte key, from the reference's `arc4.c`.")
    add("#[rustfmt::skip]")
    add("pub const KEYSTREAMS: &[([u8; 20], [u8; 32])] = &[")
    for index, key in enumerate(RC4_KEYS):
        add("    (%s, %s)," % (bytestring(key.hex()), bytestring(arc4_rows[index])))
    add("];")
    add("")
    add("/// `client_public ^ server_private mod prime`, little endian, both sides of the exchange.")
    add("#[rustfmt::skip]")
    add("pub const SHARED_SECRETS: &[([u8; 64], [u8; 64])] = &[")
    for public_hex, shared_hex in modpow_rows:
        add("    (%s, %s)," % (bytestring(public_hex), bytestring(shared_hex)))
    add("];")
    add("")
    return "\n".join(lines)


def master_hex(index):
    return MASTER_SECRETS[index].hex()


def main():
    parser = argparse.ArgumentParser(description="Generate key-exchange fixtures from GWPS's C.")
    parser.add_argument("--gwps", default=DEFAULT_GWPS, help="path to the PreSearing checkout")
    parser.add_argument("--out", default=DEFAULT_OUT, help="the Rust file to write")
    parser.add_argument(
        "--check",
        action="store_true",
        help="do not write; exit 1 if the file on disk is stale",
    )
    args = parser.parse_args()

    if not os.path.isdir(args.gwps):
        sys.exit("error: GWPS checkout not found at %s (pass --gwps)" % args.gwps)

    generated = generate(args.gwps)

    if args.check:
        current = read(args.out) if os.path.exists(args.out) else ""
        if current.replace("\r\n", "\n") != generated.replace("\r\n", "\n"):
            sys.stderr.write(
                "stale: %s does not match the reference; run tools/gen-handshake-fixtures.py\n"
                % os.path.relpath(args.out, REPO)
            )
            return 1
        print("up to date: %s" % os.path.relpath(args.out, REPO))
        return 0

    os.makedirs(os.path.dirname(args.out), exist_ok=True)
    with open(args.out, "w", encoding="utf-8", newline="\n") as handle:
        handle.write(generated)
    print("wrote %s" % os.path.relpath(args.out, REPO))
    return 0


if __name__ == "__main__":
    sys.exit(main())
