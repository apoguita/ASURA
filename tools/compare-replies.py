"""Compare two captured login replies field by field.

Both captures come from `cargo run -p asura-auth --example probe -- ... <portal-user> <session>`,
one against GWPS and one against this project's service. Walking them with the measured layout shows
which fields differ, which is the only thing that can explain a client accepting one and not the other.

Usage:  python tools/compare-replies.py .reply-gwps.bin .reply-ours.bin
"""

import sys


def walk(data):
    """Return [(header, size, fields_dict)] for a captured stream."""
    messages = []
    position = 0
    while position + 2 <= len(data):
        header = int.from_bytes(data[position:position + 2], "little")
        body = position + 2
        fields = {}

        if header == 0x0001:
            fields["salt"] = data[body:body + 4].hex()
            fields["unk"] = data[body + 4:body + 8].hex()
            size = 10
        elif header == 0x0007:
            fields["req_id"] = int.from_bytes(data[body:body + 4], "little")
            fields["character"] = data[body + 4:body + 20].hex()
            fields["unk0"] = data[body + 20:body + 24].hex()
            at = body + 24
            count = int.from_bytes(data[at:at + 2], "little")
            fields["name"] = data[at + 2:at + 2 + count * 2].decode("utf-16-le", "replace")
            at += 2 + count * 2
            ext = int.from_bytes(data[at:at + 2], "little")
            fields["extended_len"] = ext
            fields["extended"] = data[at + 2:at + 2 + ext].hex()
            size = 2 + 4 + 16 + 4 + 2 + count * 2 + 2 + ext
        elif header == 0x0016:
            fields["req_id"] = int.from_bytes(data[body:body + 4], "little")
            n = int.from_bytes(data[body + 4:body + 6], "little")
            fields["n_data"] = n
            fields["data"] = data[body + 6:body + 6 + n].hex()
            size = 2 + 4 + 2 + n
        elif header == 0x0014:
            fields["first"] = int.from_bytes(data[body:body + 4], "little")
            fields["second"] = int.from_bytes(data[body + 4:body + 8], "little")
            size = 10
        elif header == 0x0011:
            fields["req_id"] = int.from_bytes(data[body:body + 4], "little")
            fields["h0006"] = int.from_bytes(data[body + 4:body + 8], "little")
            fields["h000A"] = int.from_bytes(data[body + 8:body + 12], "little")
            fields["h000E"] = data[body + 12:body + 20].hex()
            fields["h0016"] = data[body + 20:body + 28].hex()
            fields["account"] = data[body + 28:body + 44].hex()
            fields["character"] = data[body + 44:body + 60].hex()
            fields["h003E"] = int.from_bytes(data[body + 60:body + 64], "little")
            n = int.from_bytes(data[body + 64:body + 66], "little")
            fields["n_features"] = n
            fields["features"] = data[body + 66:body + 66 + n].hex()
            fields["eula"] = data[body + 66 + n] if body + 66 + n < len(data) else None
            size = 2 + 4 + 4 + 4 + 8 + 8 + 16 + 16 + 4 + 2 + n + 1
        elif header == 0x0003:
            fields["req_id"] = int.from_bytes(data[body:body + 4], "little")
            fields["status"] = int.from_bytes(data[body + 4:body + 8], "little")
            size = 10
        elif header == 0x0000:
            fields["tick"] = int.from_bytes(data[body:body + 4], "little")
            size = 6
        else:
            messages.append((header, len(data) - position, {"raw": data[position:].hex()}))
            break

        messages.append((header, size, fields))
        position += size
    return messages


def main():
    if len(sys.argv) != 3:
        print(__doc__)
        return 2

    left = walk(open(sys.argv[1], "rb").read())
    right = walk(open(sys.argv[2], "rb").read())

    print("%-8s %-28s %s" % ("header", "GWPS", "ours"))
    interesting = []
    for index in range(max(len(left), len(right))):
        l = left[index] if index < len(left) else ("-", 0, {})
        r = right[index] if index < len(right) else ("-", 0, {})
        lh, ls, lf = l
        rh, rs, rf = r
        print("%-8s %-28s %s" % (
            ("0x%04X" % lh) if lh != "-" else "-",
            "%s (%d bytes)" % (("0x%04X" % lh) if lh != "-" else "-", ls),
            "%s (%d bytes)" % (("0x%04X" % rh) if rh != "-" else "-", rs),
        ))
        for key in sorted(set(lf) | set(rf)):
            if lf.get(key) != rf.get(key):
                interesting.append((index, lh, key, lf.get(key), rf.get(key)))

    print()
    if not interesting:
        print("the two replies are identical, field for field")
        return 0
    print("fields that differ (GWPS vs ours):")
    for index, header, key, theirs, ours in interesting:
        print("  message %d (0x%04X) field %s" % (index, header, key))
        print("    GWPS: %s" % (str(theirs)[:120]))
        print("    ours: %s" % (str(ours)[:120]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
