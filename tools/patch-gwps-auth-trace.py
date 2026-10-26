"""Add handshake tracing to the reference server, so a session log starts at byte zero.

Idempotent: re-running it does nothing if the lines are already there.
"""

path = r"C:\Users\Apo\Documents\GitHub\PreSearing\code\OpenTyria\AuthSrv.c"
src = open(path, encoding="utf-8", errors="replace").read()
original = src

version_anchor = """        conn->type = ConnType_Auth;
        conn->auth_version = version;"""
version_replacement = """        conn->type = ConnType_Auth;
        conn->auth_version = version;
        AuthSrv_TraceBytes("RECV <-", "version", data, consumed < 16 ? consumed : 16);"""

seed_anchor = """    MSG_SERVER_SEED server_seed;
    STATIC_ASSERT(sizeof(conn->master_secret) == sizeof(server_seed.seed));"""
seed_replacement = """    AuthSrv_TraceBytes("RECV <-", "client seed", data, sizeof(MSG_CLIENT_SEED));

    MSG_SERVER_SEED server_seed;
    STATIC_ASSERT(sizeof(conn->master_secret) == sizeof(server_seed.seed));"""

sent_anchor = """    if (bytes_sent != sizeof(server_seed)) {
        log_error("Failed to send %zu bytes, only %zu were sent", sizeof(server_seed), bytes_sent);"""
sent_replacement = """    AuthSrv_TraceBytes("SEND ->", "server seed", bytes, sizeof(server_seed));

    if (bytes_sent != sizeof(server_seed)) {
        log_error("Failed to send %zu bytes, only %zu were sent", sizeof(server_seed), bytes_sent);"""

for anchor, replacement in (
    (version_anchor, version_replacement),
    (seed_anchor, seed_replacement),
    (sent_anchor, sent_replacement),
):
    if replacement in src:
        print("already present:", replacement.splitlines()[0].strip()[:50])
        continue
    if anchor not in src:
        print("ANCHOR NOT FOUND:", anchor.splitlines()[0].strip()[:60])
        continue
    src = src.replace(anchor, replacement, 1)
    print("patched:", replacement.splitlines()[0].strip()[:50])

if src != original:
    open(path, "w", encoding="utf-8", newline="\n").write(src)
    print("written")
else:
    print("nothing to do")
