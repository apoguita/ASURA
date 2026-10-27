// pack-oracle -- put a message struct through the reference's own serialiser and print the bytes.
//
// Why: `GWPS_TRACE=1` prints what the reference *sent*, and a capture is a snapshot of one build. Where
// a capture and the reference's source disagree -- the `PVP_ITEM_STREAM_ADD` payloads do, see
// docs/13-load-block-bytes.md -- the question is what the source would put on the wire today. This
// answers that without a client: it fills a `GameSrvMsg` and calls the reference's `pack_msg` with the
// reference's own format table, then prints the bytes exactly as the trace would.
//
// Build (same toolchain and libraries as tools/build-oracle.ps1):
//
//   tools/build-oracle.ps1 -Source tools/pack-oracle.c -OutFile tools/pack-oracle.exe
//
// Usage:
//
//   pack-oracle <header in hex> <field values, little-endian dwords, in struct order>
//
// The values are written into the union's buffer as raw bytes at the offsets given, which is enough for a
// header whose fields are plain dwords/words; the tool prints how many bytes the format produced and their
// hex, one line per message, so two implementations can be diffed.

#define COMPILE_TOOLS 1
#include "../../PreSearing/code/OpenTyria/build_open_tyria.c"

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: %s <header hex> [byte:value ...]\n", argv[0]);
        return 2;
    }
    unsigned header = (unsigned) strtoul(argv[1], NULL, 16);
    if (header >= ARRAY_SIZE(GAME_SMSG_FORMATS)) {
        fprintf(stderr, "0x%04X is not a message header\n", header);
        return 2;
    }

    MsgFormat format = GAME_SMSG_FORMATS[header];

    // `pack-oracle fields <header>` prints the descriptor instead of packing: the reference's own field
    // list for that header, which is what a port has to lay its values out against.
    if (strcmp(argv[1], "fields") == 0) {
        if (argc < 3) {
            fprintf(stderr, "usage: %s fields <header hex>\n", argv[0]);
            return 2;
        }
        unsigned wanted = (unsigned) strtoul(argv[2], NULL, 16);
        if (wanted >= ARRAY_SIZE(GAME_SMSG_FORMATS)) {
            fprintf(stderr, "0x%04X is not a message header\n", wanted);
            return 2;
        }
        MsgFormat described = GAME_SMSG_FORMATS[wanted];
        printf("0x%04X: %zu field(s), unpacked %zu bytes\n", described.header, described.count,
               described.unpack_size);
        for (size_t idx = 0; idx < described.count; ++idx) {
            printf("  %2zu type %2u length %u\n", idx, (unsigned) described.fields[idx].type,
                   described.fields[idx].length);
        }
        return 0;
    }

    GameSrvMsg msg = {0};
    msg.header = (uint16_t) header;

    // `offset:value` pairs, so a caller can place a value at the struct offset a field lives at without
    // the tool having to know this message's field list.
    for (int idx = 2; idx < argc; ++idx) {
        unsigned offset = 0, value = 0;
        if (sscanf(argv[idx], "%u:%u", &offset, &value) != 2) {
            fprintf(stderr, "cannot read %s as offset:value\n", argv[idx]);
            return 2;
        }
        if (offset + 4 > sizeof(msg.buffer)) {
            fprintf(stderr, "offset %u is past the buffer\n", offset);
            return 2;
        }
        memcpy(&msg.buffer[offset], &value, sizeof(value));
    }

    uint8_t out[MSG_MAX_BUFFER_SIZE];
    size_t written = 0;
    if (pack_msg(format, &written, msg.buffer, sizeof(msg.buffer), out, sizeof(out)) != 0) {
        fprintf(stderr, "pack_msg failed for 0x%04X\n", header);
        return 1;
    }

    printf("PACK 0x%04X %3zu bytes :", header, written);
    for (size_t idx = 0; idx < written; ++idx) {
        printf(" %02X", out[idx]);
    }
    printf("\n");
    return 0;
}
