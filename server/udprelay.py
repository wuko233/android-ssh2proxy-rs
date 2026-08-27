#!/usr/bin/env python3
"""UDP relay for SSH2Proxy.

Runs on the SSH server, started by the client over an SSH exec channel.
Communicates over stdin/stdout with length-prefixed frames.

Request frame (client -> relay):
    [2-byte big-endian length][4-byte dst IPv4][2-byte dst port][UDP payload]

Response frame (relay -> client):
    [2-byte big-endian length][4-byte src IPv4][2-byte src port][UDP payload]
"""
import socket
import struct
import sys
import select

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.bind(("0.0.0.0", 0))

stdin = sys.stdin.buffer
stdout = sys.stdout.buffer


def read_frame():
    hdr = stdin.read(2)
    if len(hdr) < 2:
        return None
    (length,) = struct.unpack(">H", hdr)
    data = stdin.read(length)
    if len(data) < length:
        return None
    return data


while True:
    readable, _, _ = select.select([sys.stdin, sock], [], [])
    if sys.stdin in readable:
        frame = read_frame()
        if frame is None:
            break
        if len(frame) >= 6:
            ip = socket.inet_ntoa(frame[0:4])
            port = struct.unpack(">H", frame[4:6])[0]
            sock.sendto(frame[6:], (ip, port))
    if sock in readable:
        try:
            data, addr = sock.recvfrom(65535)
            src = socket.inet_aton(addr[0]) + struct.pack(">H", addr[1])
            stdout.write(struct.pack(">H", len(src) + len(data)) + src + data)
            stdout.flush()
        except Exception:
            pass