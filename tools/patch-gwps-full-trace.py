import io

path = r'C:\Users\Apo\Documents\GitHub\PreSearing\code\OpenTyria\GameSrv.c'
t = io.open(path, encoding='utf-8-sig').read()
orig_len = len(t)

helper = '''//: Dump a whole message, in hex, onto one line.
//:
//: `GameSrv_TraceMessages` used to print the first 24 bytes: enough to identify a message, not enough
//: to compare two servers, because the interesting part of an agent or a player record sits past
//: byte 24. This prints all of it -- comparing the bytes is the only way to find out why a client dies
//: against one server and not the other.
static void GameSrv_TraceBytes(const char *what, uint16_t header, const uint8_t *bytes, size_t count)
{
    static char hex[3 * MSG_MAX_BUFFER_SIZE + 1];
    size_t limit = count < MSG_MAX_BUFFER_SIZE ? count : MSG_MAX_BUFFER_SIZE;
    for (size_t idx = 0; idx < limit; ++idx) {
        snprintf(&hex[idx * 3], 4, "%02X ", bytes[idx]);
    }
    hex[limit * 3] = '\\0';
    log_info("%s 0x%04X %3zu bytes : %s", what, header, count, hex);
}

'''

anchor = 'static bool GameSrv_TraceMessages(void)'
assert anchor in t, 'trace switch not found'
end_of_trace_fn = t.index('return g_trace_messages;\n}\n')
insert_at = end_of_trace_fn + len('return g_trace_messages;\n}\n')
t = t[:insert_at] + '\n' + helper + t[insert_at:]

old_send = '''    if (GameSrv_TraceMessages()) {
        char hex[3 * 24 + 1];
        size_t n_show = written < 24 ? written : 24;
        for (size_t idx = 0; idx < n_show; ++idx) {
            snprintf(&hex[idx * 3], 4, "%02X ", dst[idx]);
        }
        hex[n_show * 3] = '\\0';
        log_info("SEND %4u 0x%04X %3zu bytes -> conn %04" PRIXPTR " : %s",
                 msg->header, msg->header, written, conn->token, hex);
    }'''
assert old_send in t, 'send trace block not found'
new_send = '''    if (GameSrv_TraceMessages()) {
        GameSrv_TraceBytes("SEND", msg->header, dst, written);
    }'''
t = t.replace(old_send, new_send)

old_recv = '''        total_consumed += consumed;
    }'''
assert old_recv in t, 'receive loop tail not found'
new_recv = '''        if (GameSrv_TraceMessages()) {
            GameSrv_TraceBytes("RECV", le16dec(input), input, consumed);
        }

        total_consumed += consumed;
    }'''
t = t.replace(old_recv, new_recv, 1)

io.open(path, 'w', encoding='utf-8', newline='\n').write(t)
print('patched', orig_len, '->', len(t))
