#!/usr/bin/env python3
"""Greedy: an example Pinch Points bot, in plain Python 3 with no packages.

    python3 greedy.py pinch://127.0.0.1:47710/7F3K-9QXA

It keeps a map of which way is home from every tile, and each time it acts
it tries a handful of signposts: for the crabs worth the most that are not
already walking home, the first few tiles on their way where a post could
turn them. It walks every crab's path with and without each candidate, and
plants the one that sends the most value home (and the least to a rival, or
a gull to its own castle). Then it rests a few ticks, so its three posts
live long enough to do their work.

It is meant to be read and changed. The protocol is docs/bot-protocol.md.
"""

import json
import socket
import sys
import time
from collections import deque

NAME = "Greedy"
VERSION = "1.0"

DIRS = {"up": (0, -1), "right": (1, 0), "down": (0, 1), "left": (-1, 0)}
LEFT = {"up": "left", "left": "down", "down": "right", "right": "up"}
RIGHT = {"up": "right", "right": "down", "down": "left", "left": "up"}
REVERSE = {"up": "down", "down": "up", "left": "right", "right": "left"}
VALUE = {"common": 1, "juvenile": 2, "giant": 10, "molting": 5, "golden": 50, "sparkling": 1}
SPEED = {"common": 12, "juvenile": 18, "giant": 7, "molting": 12, "golden": 15, "sparkling": 12}
STEPS = 24  # how far ahead a path is walked, in tiles
REST = 6  # ticks to rest after planting a post


def parse_connection_string(text):
    """pinch://HOST:PORT/KEY -> (host, port, key or None)."""
    scheme, _, rest = text.partition("://")
    if scheme not in ("pinch", "pinchs"):
        raise SystemExit("usage: greedy.py pinch://HOST:PORT[/KEY]")
    if scheme == "pinchs":
        raise SystemExit("this example speaks plain TCP; put a TLS client in front for pinchs://")
    authority, _, key = rest.partition("/")
    host, _, port = authority.rpartition(":")
    if not host:
        host, port = authority, "47710"
    return host.strip("[]"), int(port), (key or None)


class Beach:
    """The static half of a board, from `hello`."""

    def __init__(self, hello):
        board = hello["board"]
        self.w, self.h, self.wrap = board["width"], board["height"], board["wrap"]
        self.tiles = board["tiles"]
        self.hw, self.vw = board["walls"]["h"], board["walls"]["v"]
        self.seat = hello["seat"]
        self.home = None
        self.dist = {}

    def blocked(self, x, y, d):
        if d == "up":
            return self.hw[y][x] == "-"
        if d == "down":
            return self.hw[y + 1][x] == "-"
        if d == "left":
            return self.vw[y][x] == "|"
        return self.vw[y][x + 1] == "|"

    def step(self, x, y, d):
        dx, dy = DIRS[d]
        nx, ny = x + dx, y + dy
        if self.wrap:
            return nx % self.w, ny % self.h
        if 0 <= nx < self.w and 0 <= ny < self.h:
            return nx, ny
        return None

    def passable(self, x, y, d, gull=False):
        if self.blocked(x, y, d):
            return False
        n = self.step(x, y, d)
        if n is None:
            return False
        t = self.tiles[n[1]][n[0]]
        return t != "#" and not (gull and t == "K")

    def resolve(self, x, y, d, claw, gull=False):
        """Forward, else the big claw's side, else the other, else back."""
        if self.passable(x, y, d, gull):
            return d
        first, second = (RIGHT[d], LEFT[d]) if claw == "right" else (LEFT[d], RIGHT[d])
        if self.passable(x, y, first, gull):
            return first
        if self.passable(x, y, second, gull):
            return second
        return REVERSE[d]

    def aim_home(self, home):
        """Distance to `home` from every tile a crab can walk from."""
        self.home = home
        self.dist = {home: 0}
        queue = deque([home])
        while queue:
            x, y = queue.popleft()
            for d in DIRS:
                # Who can step from the neighbour into (x, y)?
                dx, dy = DIRS[d]
                px, py = x - dx, y - dy
                if self.wrap:
                    px, py = px % self.w, py % self.h
                if not (0 <= px < self.w and 0 <= py < self.h) or (px, py) in self.dist:
                    continue
                if self.tiles[py][px] in "#":
                    continue
                if self.passable(px, py, d):
                    self.dist[(px, py)] = self.dist[(x, y)] + 1
                    queue.append((px, py))

    def way_home(self, x, y):
        """The direction that gets a crab on (x, y) one tile nearer home."""
        here = self.dist.get((x, y))
        if here is None:
            return None
        best = None
        for d in DIRS:
            if self.passable(x, y, d):
                n = self.step(x, y, d)
                if n is not None and self.dist.get(n, 1 << 30) < here:
                    if best is None or self.dist[n] < self.dist[best[1]]:
                        best = (d, n)
        return best[0] if best else None


def walk(beach, x, y, d, claw, posts, castles, turnstiles, gull=False):
    """Where a walker heading `d` from (x, y) ends up: (owner, steps, tiles)."""
    seen = []
    for i in range(STEPS):
        if not beach.passable(x, y, d, gull):
            return None, i, seen
        x, y = beach.step(x, y, d)
        seen.append((x, y))
        if (x, y) in castles:
            return castles[(x, y)], i + 1, seen
        if (x, y) in turnstiles:
            d = RIGHT[d] if turnstiles[(x, y)] == "right" else LEFT[d]
        elif (x, y) in posts:
            d = posts[(x, y)]
        d = beach.resolve(x, y, d, claw, gull)
    return None, STEPS, seen


def worth(walker, owner, me, score):
    if owner is None:
        return 0.0
    if walker["gull"]:
        return -score / 2.0 if owner == me else 0.0
    return walker["value"] if owner == me else -0.3 * walker["value"]


def decide(beach, tick):
    """The best signpost to plant this tick, or None."""
    me = beach.seat
    castles = {(c["x"], c["y"]): c["owner"] for c in tick["castles"]}
    mine = [xy for xy, o in castles.items() if o == me]
    if not mine:
        return None
    if beach.home != mine[0]:
        beach.aim_home(mine[0])
    if tick["lure"]:
        return None  # every crab is walking to the lure; posts do nothing
    turnstiles = {(t["x"], t["y"]): t["next"] for t in tick["turnstiles"]}
    posts = {(p["x"], p["y"]): p["dir"] for p in tick["signposts"]}
    rival = {(p["x"], p["y"]) for p in tick["signposts"] if p["owner"] != me}
    ours = sorted((p for p in tick["signposts"] if p["owner"] == me), key=lambda p: -p["age"])
    score = tick["scores"][me]
    claw_call = tick["claw_call"]
    walkers = []
    for c in tick["crabs"]:
        value = VALUE[c["kind"]]
        if claw_call:
            value = value * 2 if c["claw"] == "right" else -value
        eta = (256 - c["progress"] + SPEED[c["kind"]] - 1) // SPEED[c["kind"]]
        walkers.append(dict(x=c["x"], y=c["y"], dir=c["dir"], claw=c["claw"], value=value, eta=eta, gull=False))
    for g in tick["gulls"]:
        if g["state"] == "walking":
            walkers.append(dict(x=g["x"], y=g["y"], dir=g["dir"], claw=g["claw"], value=0, eta=32, gull=True))

    def outcome(w, with_posts):
        owner, steps, tiles = walk(beach, w["x"], w["y"], w["dir"], w["claw"], with_posts, castles, turnstiles, w["gull"])
        return worth(w, owner, me, score), tiles

    base = []
    for w in walkers:
        value, tiles = outcome(w, posts)
        base.append((value, set(tiles), tiles))

    # Candidates: the first few sand tiles on the way of whatever is not
    # already doing what we want, each pointed home (or away, for a gull).
    candidates = set()
    order = sorted(range(len(walkers)), key=lambda i: -(abs(walkers[i]["value"]) + (score / 2 if walkers[i]["gull"] else 0)) / (1 + walkers[i]["eta"]))
    for i in order[:12]:
        w = walkers[i]
        if not w["gull"] and base[i][0] >= w["value"] > 0:
            continue
        if w["gull"] and base[i][0] == 0:
            continue
        tried = 0
        for (x, y) in base[i][2][:8]:
            if beach.tiles[y][x] != "." or (x, y) in rival:
                continue
            if w["gull"]:
                for d in DIRS:
                    if d != posts.get((x, y)):
                        candidates.add((x, y, d))
            else:
                d = beach.way_home(x, y)
                if d and posts.get((x, y)) != d:
                    candidates.add((x, y, d))
            tried += 1
            if tried >= 3:
                break

    best, best_gain = None, 0.4
    for (x, y, d) in candidates:
        trial = dict(posts)
        trial[(x, y)] = d
        evicted = None
        if (x, y) not in posts and len(ours) >= 3:
            evicted = (ours[0]["x"], ours[0]["y"])
            trial.pop(evicted, None)
        gain = 0.0
        for i, w in enumerate(walkers):
            touched = (x, y) in base[i][1] or (evicted is not None and evicted in base[i][1])
            if touched:
                gain += outcome(w, trial)[0] - base[i][0]
        if gain > best_gain:
            best, best_gain = (x, y, d), gain
    if best is None:
        return None
    return {"act": "place", "x": best[0], "y": best[1], "dir": best[2], "note": "gain %.1f" % best_gain}


def main():
    if len(sys.argv) < 2:
        raise SystemExit("usage: greedy.py pinch://HOST:PORT[/KEY] [name]")
    host, port, key = parse_connection_string(sys.argv[1])
    name = sys.argv[2] if len(sys.argv) > 2 else NAME
    sock = socket.create_connection((host, port))
    # Nagle's algorithm can hold a small line for 40 ms, longer than the
    # whole deadline: switch it off first.
    sock.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)
    stream = sock.makefile("rw", encoding="utf-8", newline="\n")

    def send(msg):
        stream.write(json.dumps(msg, separators=(",", ":")) + "\n")
        stream.flush()

    register = {"type": "register", "protocol": 1, "name": name, "version": VERSION}
    if key:
        register["key"] = key
    send(register)
    beaches = {}
    for line in stream:
        msg = json.loads(line)
        kind = msg.get("type")
        if kind == "tick":
            beach = beaches.get(msg["game"])
            reply = beach and decide(beach, msg)
            if reply:
                reply.update(game=msg["game"], tick=msg["tick"], wait=REST)
            else:
                reply = {"game": msg["game"], "tick": msg["tick"], "act": "none"}
            send(reply)
        elif kind == "ping":
            send({"type": "pong", "id": msg["id"]})
        elif kind == "hello":
            beaches[msg["game"]] = Beach(msg)
            send({"type": "ready", "game": msg["game"]})
        elif kind == "end":
            beaches.pop(msg["game"], None)
            print("game %d: placed %d, scores %s" % (msg["game"], msg["placing"], msg["scores"]), flush=True)
        elif kind == "registered":
            print("registered as %s" % msg["name"], flush=True)
        elif kind == "error":
            print("error: %s" % msg["message"], file=sys.stderr, flush=True)
            if msg.get("fatal"):
                return


if __name__ == "__main__":
    main()
