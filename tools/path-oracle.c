// path-oracle -- run the reference's own path finder over a list of cases, so a port of it can be judged.
//
// Why this lives here rather than in the reference: the reference must not be modified, and `GmPaths.c`
// has no tool of its own (`GmPathHeap`/`GmPaths` are only reached through `GameSrv_HandleMoveToCoord`).
// So this compiles the reference's own single translation unit, unmodified, exactly the way the
// reference's own tools do -- `#define COMPILE_TOOLS 1` and include it -- and calls `PathFinding`
// directly. Nothing under `PreSearing/` is written to; only its sources, headers and prebuilt static
// libraries are read.
//
// Output, per case, one CSV row:
//
//   blob,src_x,src_y,src_plane,dst_x,dst_y,dst_plane,result,count[,trap_id,x,y,plane]...
//
// Floats are printed as their raw IEEE-754 bit patterns in hex, not as decimals: the point is to compare
// two implementations bit for bit, and a decimal form would hide a one-ulp difference or invent one.
//
// Build (with the MSVC toolchain the reference was built with):
//
//   call "C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Auxiliary\Build\vcvars64.bat"
//   cl /nologo /O2 /W0 /DCOMPILE_TOOLS ^
//      /I ..\..\PreSearing\code\OpenTyria /I ..\..\PreSearing\deps\mbedtls-3.6.0\include ^
//      path-oracle.c /Fe:path-oracle.exe /Fo:path-oracle.obj ^
//      /link ..\..\PreSearing\build\RelWithDebInfo\gwps_sqlite.lib ^
//           ..\..\PreSearing\build\mbedtls\library\RelWithDebInfo\mbedtls.lib ^
//           ..\..\PreSearing\build\mbedtls\library\RelWithDebInfo\mbedx509.lib ^
//           ..\..\PreSearing\build\mbedtls\library\RelWithDebInfo\mbedcrypto.lib ^
//           ..\..\PreSearing\build\mbedtls\3rdparty\everest\RelWithDebInfo\everest.lib ^
//           ..\..\PreSearing\build\mbedtls\3rdparty\p256-m\RelWithDebInfo\p256m.lib ^
//           Ws2_32.lib bcrypt.lib ntdll.lib
//
// Usage:
//
//   path-oracle <Gw.dat> <cases file>
//
// The cases file is one case per line: `blob src_x src_y src_plane dst_x dst_y dst_plane`, where `blob` is
// a decimal or 0x-prefixed file id and the rest are decimal numbers. `tools/gen-path-cases.py` writes it;
// `tools/parity-path.py --check` runs this and `cargo run --example path-census` over the same file.

#define COMPILE_TOOLS 1
#include "../../PreSearing/code/OpenTyria/build_open_tyria.c"

#define MAX_CACHED_MAPS 64

typedef struct CachedMap {
    uint32_t      file_id;
    GmMapContext  context;
} CachedMap;

static CachedMap cached_maps[MAX_CACHED_MAPS];
static size_t    cached_count;

static uint32_t float_bits(float value)
{
    uint32_t bits;
    memcpy(&bits, &value, sizeof(bits));
    return bits;
}

// Import a blob once and keep it: every case for a map then costs a path find, not a decompression.
static GmMapContext *get_map(FileArchive *archive, uint32_t file_id)
{
    for (size_t idx = 0; idx < cached_count; ++idx) {
        if (cached_maps[idx].file_id == file_id) {
            return &cached_maps[idx].context;
        }
    }
    if (cached_count == MAX_CACHED_MAPS) {
        fprintf(stderr, "too many distinct maps\n");
        return NULL;
    }

    array_uint8_t content = {0};
    if (FaGetFile(archive, file_id, &content) != 0) {
        fprintf(stderr, "0x%08X: not found or failed to read\n", file_id);
        array_free(&content);
        return NULL;
    }

    CachedMap *slot = &cached_maps[cached_count];
    memset(slot, 0, sizeof(*slot));
    slot->file_id = file_id;

    slice_uint8_t bytes = { .ptr = content.ptr, .len = content.len };
    int err = GmImportMap(&slot->context, bytes);
    array_free(&content);
    if (err != 0) {
        fprintf(stderr, "0x%08X: GmImportMap failed (%d)\n", file_id, err);
        return NULL;
    }
    ++cached_count;
    return &slot->context;
}

int main(int argc, char **argv)
{
    if (argc < 3) {
        fprintf(stderr, "usage: %s <Gw.dat> <cases file>\n", argv[0]);
        return 2;
    }

    FileArchive archive = {0};
    if (FaOpen(&archive, argv[1]) != 0) {
        fprintf(stderr, "failed to open '%s'\n", argv[1]);
        return 1;
    }

    FILE *cases = fopen(argv[2], "r");
    if (cases == NULL) {
        fprintf(stderr, "cannot read '%s'\n", argv[2]);
        FaClose(&archive);
        return 1;
    }

    printf("blob,src_x,src_y,src_plane,dst_x,dst_y,dst_plane,result,count,waypoints\n");

    char line[512];
    size_t cases_run = 0, solved = 0;
    while (fgets(line, sizeof(line), cases) != NULL) {
        char *cursor = line;
        while (*cursor == ' ' || *cursor == '\t') {
            ++cursor;
        }
        if (*cursor == '#' || *cursor == '\n' || *cursor == '\r' || *cursor == 0) {
            continue;
        }

        char blob_text[32];
        GmPos src = {0}, dst = {0};
        unsigned src_plane = 0, dst_plane = 0;
        if (sscanf(cursor, "%31s %f %f %u %f %f %u",
                   blob_text, &src.x, &src.y, &src_plane,
                   &dst.x, &dst.y, &dst_plane) != 7) {
            fprintf(stderr, "cannot parse case: %s", cursor);
            continue;
        }
        src.plane = (uint16_t) src_plane;
        dst.plane = (uint16_t) dst_plane;

        uint32_t file_id = (uint32_t) strtoul(blob_text, NULL, 0);
        GmMapContext *context = get_map(&archive, file_id);
        if (context == NULL) {
            continue;
        }

        WaypointArray waypoints = {0};
        bool ok = PathFinding(&context->path, src, dst, &waypoints);

        printf("0x%08X,%08X,%08X,%u,%08X,%08X,%u,%d,%zu,",
               file_id,
               float_bits(src.x), float_bits(src.y), (unsigned) src.plane,
               float_bits(dst.x), float_bits(dst.y), (unsigned) dst.plane,
               ok ? 1 : 0, waypoints.len);
        for (size_t idx = 0; idx < waypoints.len; ++idx) {
            Waypoint *waypoint = &waypoints.ptr[idx];
            if (idx != 0) {
                printf(";");
            }
            printf("%u,%08X,%08X,%u",
                   waypoint->trap_id,
                   float_bits(waypoint->pos.x),
                   float_bits(waypoint->pos.y),
                   (unsigned) waypoint->pos.plane);
        }
        printf("\n");

        ++cases_run;
        if (ok) {
            ++solved;
        }
        array_free(&waypoints);
    }

    fclose(cases);
    fprintf(stderr, "done: %zu case(s), %zu solved\n", cases_run, solved);

    for (size_t idx = 0; idx < cached_count; ++idx) {
        GmMapContext_Free(&cached_maps[idx].context);
    }
    FaClose(&archive);
    return 0;
}
