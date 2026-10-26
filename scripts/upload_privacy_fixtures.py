"""Synthetic PNG privacy and stream fixtures; stdlib only, no private image data."""
from pathlib import Path
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "spikes/own-renderer/e2e/uploads"


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def make():
    signature = b"\x89PNG\r\n\x1a\n"
    head = chunk(b"IHDR", struct.pack(">IIBBBBB", 2, 2, 8, 6, 0, 0, 0))
    end = chunk(b"IEND", b"")
    raw = (b"\0" + b"\x80\x40\x20\xff" * 2) * 2
    encoded = zlib.compress(raw)
    idat = chunk(b"IDAT", encoded)
    marker = b"PLERIS-PRIVATE-COLOR-20261025".ljust(32, b"!")
    chrm = chunk(b"cHRM", struct.pack(">8I", 31270, 32900, 64000, 33000, 30000, 60000, 15000, 6000))
    def image(*parts):
        return signature + head + b"".join(parts) + end
    cases = {
        "clean": image(idat),
        "late-chrm": image(idat, chunk(b"cHRM", marker)),
        "duplicate-chrm": image(chrm, chunk(b"cHRM", marker), idat),
        "idat-suffix": image(chunk(b"IDAT", encoded + marker)),
        "later-idat-suffix": image(idat, chunk(b"IDAT", marker)),
        "decoded-suffix": image(chunk(b"IDAT", zlib.compress(raw + marker))),
        "split-idat": image(chunk(b"IDAT", b""), *[chunk(b"IDAT", bytes([b])) for b in encoded], chunk(b"IDAT", b"")),
        "nonconsecutive-idat": image(chunk(b"IDAT", encoded[:3]), chunk(b"tEXt", b"a\0b"), chunk(b"IDAT", encoded[3:])),
        "nonconsecutive-complete-idat": image(idat, chunk(b"tEXt", b"a\0b"), chunk(b"IDAT", marker)),
    }
    def control(seq, w=2, h=2):
        return chunk(b"fcTL", struct.pack(">IIIIIHHBB", seq, w, h, 0, 0, 1, 10, 0, 0))

    def frame(seq, payload):
        return chunk(b"fdAT", struct.pack(">I", seq) + payload)

    def actl(n):
        return chunk(b"acTL", struct.pack(">II", n, 0))
    cases.update({
        "apng-included": image(actl(2), control(0), idat, control(1), frame(2, encoded)),
        "apng-excluded": image(actl(1), idat, control(0), frame(1, encoded)),
        "apng-suffix": image(actl(1), idat, control(0), frame(1, encoded + marker)),
        "orphan-fdat": image(idat, frame(0, encoded)),
        "apng-sequence": image(actl(1), idat, control(0), frame(2, encoded)),
        "apng-rectangle": image(actl(1), idat, control(0, 3), frame(1, zlib.compress(bytes(26)))),
        "apng-count": image(actl(2), idat, control(0), frame(1, encoded)),
        "apng-interleaved": image(actl(1), idat, control(0), frame(1, b""), *[
            frame(i + 2, bytes([b])) + chunk(b"tEXt", b"a\0discarded") for i, b in enumerate(encoded)
        ], frame(len(encoded) + 2, b"")),
        "apng-split": image(actl(1), idat, control(0), frame(1, b""), *[
            frame(i + 2, bytes([b])) for i, b in enumerate(encoded)
        ], frame(len(encoded) + 2, b"")),
    })
    profile = (OUT / "sRGB2014.icc").read_bytes()
    count = struct.unpack_from(">I", profile, 128)[0]
    tags = [struct.unpack_from(">4sII", profile, 132 + 12 * i) for i in range(count)]
    # Altered ICC fixtures must not retain the original identification/copyright.
    entries = [(sig, profile[at:at + size]) for sig, at, size in tags
               if sig not in (b"desc", b"cprt", b"dmnd", b"dmdd", b"vued")]
    neutral_header = bytearray(profile[:128])
    for start, end_at in [(4, 8), (24, 36), (40, 44), (48, 64), (80, 100), (100, 128)]:
        neutral_header[start:end_at] = bytes(end_at - start)
    entries.append((b"rXYZ", b"XYZ " + bytes(4) + b"ICC-PRIVATE!"))
    dup = bytearray(neutral_header + struct.pack(">I", len(entries)) + bytes(12 * len(entries)))
    for i, (sig, data) in enumerate(entries):
        dup.extend(bytes((-len(dup)) % 4))
        at = len(dup)
        dup.extend(data)
        struct.pack_into(">4sII", dup, 132 + 12 * i, sig, at, len(data))
    dup.extend(bytes((-len(dup)) % 4))
    struct.pack_into(">I", dup, 0, len(dup))
    cases["duplicate-icc"] = image(chunk(b"iCCP", b"private\0\0" + zlib.compress(dup)), idat)
    cases["valid-icc"] = image(chunk(b"iCCP", b"sRGB\0\0" + zlib.compress(profile)), idat)
    cases["truncated-zlib"] = image(chunk(b"IDAT", encoded[:-1]))
    cases["bad-adler"] = image(chunk(b"IDAT", encoded[:-1] + bytes([encoded[-1] ^ 1])))
    for depth, colour in [(1, 0), (8, 0), (16, 0), (8, 2), (16, 2), (16, 4), (16, 6), (1, 3), (2, 3), (4, 3)]:
        channels = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}[colour]
        layout_header = chunk(b"IHDR", struct.pack(">IIBBBBB", 3, 2, depth, colour, 0, 0, 0))
        filtered = bytes((((3 * depth * channels + 7) // 8) + 1) * 2)
        palette = chunk(b"PLTE", bytes([0, 0, 0, 255, 255, 255])) + chunk(b"tRNS", bytes([0, 255])) if colour == 3 else b""
        cases[f"layout-{colour}-{depth}"] = signature + layout_header + palette + chunk(b"IDAT", zlib.compress(filtered)) + end
    cases["adam7-tiny"] = signature + chunk(b"IHDR", struct.pack(">IIBBBBB", 1, 1, 16, 6, 0, 0, 1)) + chunk(b"IDAT", zlib.compress(bytes(9))) + end
    for name, data in cases.items():
        (OUT / ("privacy-" + name + ".png")).write_bytes(data)


if __name__ == "__main__":
    make()