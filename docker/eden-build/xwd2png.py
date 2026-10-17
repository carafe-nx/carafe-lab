import struct
import sys
import zlib


def read_xwd(path):
    with open(path, "rb") as f:
        data = f.read()
    header = struct.unpack(">25I", data[:100])
    header_size = header[0]
    width, height = header[4], header[5]
    bits_per_pixel = header[11]
    bytes_per_line = header[12]
    ncolors = header[19]
    offset = header_size + ncolors * 12
    if bits_per_pixel != 32:
        raise SystemExit(f"{path}: {bits_per_pixel} bits per pixel, expected 32")
    rows = []
    for y in range(height):
        line = data[offset + y * bytes_per_line : offset + y * bytes_per_line + width * 4]
        rgb = bytearray(width * 3)
        rgb[0::3] = line[2::4]
        rgb[1::3] = line[1::4]
        rgb[2::3] = line[0::4]
        rows.append(b"\x00" + bytes(rgb))
    return width, height, b"".join(rows)


def write_png(path, width, height, raw):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xFFFFFFFF)

    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(raw, 6)))
        f.write(chunk(b"IEND", b""))


if __name__ == "__main__":
    width, height, raw = read_xwd(sys.argv[1])
    write_png(sys.argv[2], width, height, raw)
