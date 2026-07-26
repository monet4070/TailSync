#!/usr/bin/env python3
"""
Simulate a Tailscale peer sending clipboard content to the local TailSync
TCP server.  Uses the TSYN binary protocol exactly as defined in
src-tauri/src/protocol.rs.

Usage:
    python3 scripts/test_peer.py "Hello from fake peer"
    python3 scripts/test_peer.py --port 19888 "some text"

Requirements: Python 3.8+, blake3 (`pip3 install blake3`).

This sends:
  1. HandshakeReq  frame
  2. Waits for HandshakeAck
  3. TextPayload   frame
"""

import argparse
import json
import socket
import struct
import sys
import time

MAGIC = b"TSYN"
VERSION = 0x01
HEADER_SIZE = 16
CHECKSUM_SIZE = 32

# Command codes
CMD_HANDSHAKE_REQ = 0x0001
CMD_HANDSHAKE_ACK = 0x0002
CMD_TEXT_PAYLOAD  = 0x0101


def blake3_hash(data: bytes) -> bytes:
    """Return 32-byte blake3 hash of `data` (raw bytes, not hex)."""
    try:
        import blake3
        return blake3.blake3(data).digest()
    except ImportError:
        import hashlib
        # Pure-python fallback — blake3 is strongly recommended
        print("[warn] blake3 not installed, using SHA-256 as fallback (peer will reject!)")
        print("[warn] run: pip3 install blake3")
        return hashlib.sha256(data).digest()


def encode_frame(command: int, payload: bytes, sequence: int = 0) -> bytes:
    """Encode a TSYN frame."""
    payload_len = len(payload)
    header = struct.pack(
        ">4s B B H I I",
        MAGIC,           # 4 bytes
        VERSION,         # 1 byte
        0x00,            # flags
        command,         # 2 bytes BE
        sequence,        # 4 bytes BE
        payload_len,     # 4 bytes BE
    )
    checksum = blake3_hash(header + payload)
    return header + payload + checksum


def decode_frame(data: bytes) -> tuple[int, bytes]:
    """Decode a TSYN frame. Returns (command, payload)."""
    if len(data) < HEADER_SIZE + CHECKSUM_SIZE:
        raise ValueError(f"Frame too short: {len(data)} bytes")
    # Parse header
    magic, ver, flags, cmd, seq, plen = struct.unpack_from(
        ">4s B B H I I", data, 0
    )
    if magic != MAGIC:
        raise ValueError(f"Bad magic: {magic!r}")
    if ver != VERSION:
        raise ValueError(f"Bad version: {ver}")
    payload_start = HEADER_SIZE
    payload_end = payload_start + plen
    payload = data[payload_start:payload_end]
    expected_csum = data[payload_end:payload_end + CHECKSUM_SIZE]
    # Verify checksum
    computed = blake3_hash(data[:payload_end])
    if computed != expected_csum:
        raise ValueError("Checksum mismatch")
    return cmd, payload


def main():
    parser = argparse.ArgumentParser(description="TailSync test peer")
    parser.add_argument("text", help="Text to send as clipboard content")
    parser.add_argument("--host", default="127.0.0.1", help="Target host")
    parser.add_argument("--port", type=int, default=19888, help="Target port")
    args = parser.parse_args()

    addr = (args.host, args.port)
    print(f"Connecting to {args.host}:{args.port}...")

    sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    sock.settimeout(10)

    try:
        sock.connect(addr)
        print("TCP connected")

        # ── Step 1: Send HandshakeReq ─────────────────────────────
        handshake = json.dumps({
            "hostname": "test-peer.local",
            "tailscale_ip": "100.64.0.99",
        }).encode()
        hs_frame = encode_frame(CMD_HANDSHAKE_REQ, handshake)
        sock.sendall(hs_frame)
        print(f"→ HandshakeReq ({len(hs_frame)} bytes)")

        # ── Step 2: Read HandshakeAck ──────────────────────────────
        # Read header first to determine payload length
        header = recv_exact(sock, HEADER_SIZE)
        _, _, _, cmd, _, plen = struct.unpack_from(">4s B B H I I", header, 0)
        payload = recv_exact(sock, plen)
        checksum = recv_exact(sock, CHECKSUM_SIZE)

        if cmd != CMD_HANDSHAKE_ACK:
            print(f"✗ Expected HandshakeAck (0x{CMD_HANDSHAKE_ACK:04x}), got 0x{cmd:04x}")
            sys.exit(1)

        ack_data = json.loads(payload)
        print(f"← HandshakeAck: accepted={ack_data.get('accepted')}, version={ack_data.get('version')}")

        if not ack_data.get("accepted"):
            print("✗ Peer rejected handshake")
            sys.exit(1)

        # ── Step 3: Send TextPayload ───────────────────────────────
        text_bytes = args.text.encode("utf-8")
        text_frame = encode_frame(CMD_TEXT_PAYLOAD, text_bytes)
        sock.sendall(text_frame)
        print(f"→ TextPayload: {args.text!r} ({len(text_bytes)} bytes)")

        print()
        print("✓ Text payload sent successfully!")
        print()
        print("Now check:")
        print("  1. Your system clipboard should contain the text above")
        print("  2. TailSync History window should show a new entry")
        print("     with source_peer = '127.0.0.1:19888' or similar")

        time.sleep(1)

    except socket.timeout:
        print("✗ Connection timed out. Is TailSync running?")
        sys.exit(1)
    except ConnectionRefusedError:
        print("✗ Connection refused. Is TailSync running on port", args.port, "?")
        sys.exit(1)
    except Exception as e:
        print(f"✗ Error: {e}")
        sys.exit(1)
    finally:
        sock.close()


def recv_exact(sock: socket.socket, n: int) -> bytes:
    """Receive exactly n bytes from socket."""
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("Connection closed")
        buf += chunk
    return buf


if __name__ == "__main__":
    main()
