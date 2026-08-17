#!/usr/bin/env python3
"""Synthesized one-shot effects for Pinch Points (stdlib + ffmpeg).

Generates the second batch of chiptune effects; the original seven
(place/remove/bank/eat/raid/win/lose) plus theme.wav predate this script
and are kept as-is in assets/sounds.

One-shots stay WAV. The music loops are minutes long, so they ship as
OGG Vorbis (~10x smaller); encoding goes through ffmpeg, the one
non-stdlib requirement.
"""
import math
import os
import struct
import subprocess
import tempfile
import wave

RATE = 44100


def _write_wav(path, samples):
    with wave.open(path, "wb") as f:
        f.setnchannels(1)
        f.setsampwidth(2)
        f.setframerate(RATE)
        clipped = (max(-1.0, min(1.0, s)) for s in samples)
        f.writeframes(b"".join(struct.pack("<h", int(s * 32000)) for s in clipped))


def write(name, samples):
    _write_wav(f"assets/sounds/{name}.wav", samples)
    print(name)


def write_music(name, samples):
    """Same synth output, encoded to OGG Vorbis via a temporary WAV.
    Quality 4 is transparent for these square/triangle voices."""
    fd, tmp = tempfile.mkstemp(suffix=".wav")
    os.close(fd)
    try:
        _write_wav(tmp, samples)
        subprocess.run(
            ["ffmpeg", "-y", "-loglevel", "error", "-i", tmp,
             "-c:a", "libvorbis", "-q:a", "4", f"assets/sounds/{name}.ogg"],
            check=True,
        )
    finally:
        os.unlink(tmp)
    print(name)


def env(i, n, attack=0.01, release=0.25):
    """Linear attack, exponential-ish release."""
    t = i / n
    a = min(1.0, t / attack) if attack > 0 else 1.0
    r = (1.0 - t) ** (1.0 / release) if release > 0 else 1.0
    return a * r


def square(phase, duty=0.5):
    return 1.0 if (phase % 1.0) < duty else -1.0


def triangle(phase):
    p = phase % 1.0
    return 4.0 * p - 1.0 if p < 0.5 else 3.0 - 4.0 * p


def tone(freqs, dur, wave_fn=square, vol=0.5, bend=1.0, vibrato=0.0):
    """One note or a slide across `freqs` (start->end), with optional vibrato."""
    n = int(dur * RATE)
    f0, f1 = (freqs, freqs) if isinstance(freqs, (int, float)) else freqs
    out = []
    phase = 0.0
    for i in range(n):
        t = i / n
        f = (f0 + (f1 - f0) * t) * (bend ** t)
        if vibrato:
            f *= 1.0 + math.sin(t * dur * 2 * math.pi * 30) * vibrato
        phase += f / RATE
        out.append(wave_fn(phase) * vol * env(i, n))
    return out


def noise(dur, vol=0.4, lowpass=0.2, fade_in=False):
    """Cheap filtered white noise (LCG), optional swelling envelope."""
    n = int(dur * RATE)
    seed = 0x2545F491
    out, prev = [], 0.0
    for i in range(n):
        seed = (seed * 1103515245 + 12345) & 0x7FFFFFFF
        white = seed / 0x3FFFFFFF - 1.0
        prev += lowpass * (white - prev)
        e = (i / n) if fade_in else env(i, n, attack=0.02)
        out.append(prev * vol * e)
    return out


def mix(*layers):
    n = max(len(l) for l in layers)
    return [sum(l[i] if i < len(l) else 0.0 for l in layers) for i in range(n)]


# Gull takeoff: a rising whoosh of air.
write("takeoff", mix(
    noise(0.32, vol=0.5, lowpass=0.35, fade_in=True),
    tone((180, 720), 0.32, triangle, vol=0.18),
))

# Gull screech: two harsh falling squawks.
write("screech", (
    tone((1500, 950), 0.13, lambda p: square(p, 0.3), vol=0.30, vibrato=0.04)
    + [0.0] * int(0.04 * RATE)
    + tone((1350, 800), 0.16, lambda p: square(p, 0.3), vol=0.26, vibrato=0.04)
))

# Tide-event roulette: a curious chromatic question mark.
write("event", (
    tone(523, 0.09, triangle, vol=0.4)
    + tone(622, 0.09, triangle, vol=0.4)
    + tone(740, 0.09, triangle, vol=0.4)
    + tone(880, 0.22, triangle, vol=0.45, vibrato=0.02)
))

# Golden crab banked: a jackpot arpeggio over two octaves.
GOLDEN = [523, 659, 784, 1047, 1319, 1568, 2093]
write("golden", sum((tone(f, 0.07, triangle, vol=0.42) for f in GOLDEN),
                    []) + tone(2093, 0.18, triangle, vol=0.4))

# Castle tier up: a proud three-note fanfare.
write("tier", (
    tone(392, 0.09, square, vol=0.28)
    + tone(523, 0.09, square, vol=0.28)
    + tone(659, 0.2, square, vol=0.32)
))

# Final surge: an urgent two-tone alarm over surf.
write("surge", mix(
    noise(0.65, vol=0.28, lowpass=0.12),
    tone(440, 0.16, square, vol=0.2) + tone(587, 0.16, square, vol=0.2)
    + tone(440, 0.16, square, vol=0.2) + tone(587, 0.16, square, vol=0.2),
))

# Round-end horn: a long falling fifth.
write("horn", mix(
    tone((392, 261), 0.7, square, vol=0.3),
    tone((196, 130), 0.7, triangle, vol=0.3),
))

# Placement denied: a short dull double-knock.
write("denied", (
    tone(140, 0.06, square, vol=0.30)
    + [0.0] * int(0.03 * RATE)
    + tone(110, 0.09, square, vol=0.26)
))

# --- theme B: a second seaside loop so the first one can breathe -------------
# 16 bars at 132 bpm, waltz-y 3/4: triangle melody over a square bass,
# different key (D minor-ish) and feel from theme.wav.
NOTE = {"D3": 146.83, "F3": 174.61, "G3": 196.0, "A3": 220.0, "Bb3": 233.08,
        "C4": 261.63, "D4": 293.66, "E4": 329.63, "F4": 349.23, "G4": 392.0,
        "A4": 440.0, "Bb4": 466.16, "C5": 523.25, "D5": 587.33, "R": 0.0}
BEAT = 60.0 / 132.0

def voice(seq, wave_fn, vol):
    out = []
    for name, beats in seq:
        dur = beats * BEAT
        if name == "R":
            out += [0.0] * int(dur * RATE)
        else:
            out += tone(NOTE[name], dur, wave_fn, vol=vol)
    return out

MELODY = [
    ("D4", 1), ("F4", 1), ("A4", 1), ("D5", 2), ("C5", 1),
    ("Bb4", 1), ("A4", 1), ("F4", 1), ("G4", 3),
    ("A4", 1), ("Bb4", 1), ("C5", 1), ("D5", 2), ("A4", 1),
    ("Bb4", 1), ("G4", 1), ("E4", 1), ("F4", 3),
    ("F4", 1), ("A4", 1), ("C5", 1), ("D5", 2), ("F4", 1),
    ("G4", 1), ("A4", 1), ("Bb4", 1), ("A4", 3),
    ("D5", 1), ("C5", 1), ("Bb4", 1), ("A4", 1), ("G4", 1), ("E4", 1),
    ("D4", 3), ("R", 3),
]
BASS = [
    ("D3", 3), ("D3", 3), ("Bb3", 3), ("Bb3", 3),
    ("F3", 3), ("F3", 3), ("G3", 3), ("G3", 3),
    ("D3", 3), ("D3", 3), ("Bb3", 3), ("Bb3", 3),
    ("F3", 3), ("A3", 3), ("D3", 3), ("D3", 3),
]
melody = voice(MELODY, triangle, 0.30)
bass = voice(BASS, lambda p: square(p, 0.35), 0.16)
surf = noise(len(melody) / RATE, vol=0.05, lowpass=0.04)
write_music("theme_b", mix(melody, bass, surf))

# --- theme C: bright market-day major, 4/4 at 140 bpm ------------------------
NOTE.update({"E3": 164.81, "C3": 130.81, "G5": 783.99, "E5": 659.26})
BEAT = 60.0 / 140.0
MELODY_C = [
    ("C4", 1), ("E4", 1), ("G4", 1), ("C5", 1),
    ("G4", 1), ("E4", 1), ("G4", 2),
    ("A4", 1), ("G4", 1), ("E4", 1), ("D4", 1), ("C4", 2), ("R", 2),
    ("E4", 1), ("G4", 1), ("C5", 1), ("E5", 1),
    ("D5", 1), ("C5", 1), ("A4", 2),
    ("G4", 1), ("A4", 1), ("C5", 1), ("D5", 1), ("C5", 2), ("R", 2),
]
BASS_C = [
    ("C3", 2), ("G3", 2), ("A3", 2), ("G3", 2),
    ("F3", 2), ("C3", 2), ("G3", 2), ("C3", 2),
    ("C3", 2), ("G3", 2), ("A3", 2), ("E3", 2),
    ("F3", 2), ("G3", 2), ("C3", 2), ("C3", 2),
]
melody = voice(MELODY_C, triangle, 0.30)
bass = voice(BASS_C, lambda p: square(p, 0.3), 0.15)
surf = noise(len(melody) / RATE, vol=0.04, lowpass=0.05)
write_music("theme_c", mix(melody, bass, surf))

# --- theme D: slow moonlit minor, 4/4 at 96 bpm ------------------------------
NOTE.update({"B3": 246.94, "E4b": 311.13})
BEAT = 60.0 / 96.0
MELODY_D = [
    ("E4", 2), ("G4", 1), ("A4", 1), ("B4", 3), ("A4", 1),
    ("G4", 2), ("E4", 2), ("D4", 3), ("R", 1),
    ("E4", 2), ("G4", 1), ("B4", 1), ("D5", 3), ("B4", 1),
    ("A4", 2), ("G4", 1), ("A4", 1), ("E4", 3), ("R", 1),
]
NOTE["B4"] = 493.88
BASS_D = [
    ("E3", 4), ("C3", 4), ("G3", 4), ("D3", 4),
    ("E3", 4), ("C3", 4), ("A3", 4), ("E3", 4),
]
melody = voice(MELODY_D, triangle, 0.26)
bass = voice(BASS_D, lambda p: square(p, 0.4), 0.13)
surf = noise(len(melody) / RATE, vol=0.06, lowpass=0.03)
write_music("theme_d", mix(melody, bass, surf))

# --- theme E: sunny shallows, G major pentatonic, 4/4 at 126 bpm -------------
# The first of three loops added to stretch the rotation. Its signature is
# rhythmic rather than melodic: the accompaniment plucks on the off-beats
# instead of holding a bass note per bar, which makes it read as a different
# band from B/C/D even at the same tempo.
NOTE.update({"G5": 783.99, "E5": 659.26, "B4": 493.88, "B3": 246.94})
BEAT = 60.0 / 126.0

MELODY_E = [
    ("G4", 1), ("B4", 1), ("D5", 2),
    ("E5", 1), ("D5", 1), ("B4", 2),
    ("A4", 1), ("B4", 1), ("D5", 1), ("B4", 1),
    ("G4", 3), ("R", 1),
    ("B4", 1), ("D5", 1), ("E5", 2),
    ("G5", 1), ("E5", 1), ("D5", 2),
    ("E5", 1), ("D5", 1), ("B4", 1), ("A4", 1),
    ("G4", 3), ("R", 1),
    ("D5", 1), ("E5", 1), ("G5", 2),
    ("E5", 1), ("D5", 1), ("B4", 2),
    ("A4", 1), ("G4", 1), ("A4", 1), ("B4", 1),
    ("D5", 3), ("R", 1),
    ("B4", 1), ("A4", 1), ("G4", 1), ("E4", 1),
    ("D4", 1), ("E4", 1), ("G4", 2),
    ("A4", 1), ("B4", 1), ("A4", 1), ("G4", 1),
    ("G4", 3), ("R", 1),
]
# One chord per bar, as (root, off-beat note).
CHORDS_E = [
    ("G3", "B4"), ("C3", "E4"), ("D3", "A4"), ("G3", "B4"),
    ("E3", "G4"), ("C3", "E4"), ("D3", "A4"), ("G3", "B4"),
    ("G3", "D5"), ("E3", "B4"), ("C3", "G4"), ("D3", "A4"),
    ("E3", "G4"), ("C3", "E4"), ("D3", "A4"), ("G3", "B4"),
]

def offbeat(chords, wave_fn, root_vol, pluck_vol):
    """A bar of root on 1 and two plucks on the off-beats of 2 and 3."""
    root, pluck = [], []
    for note, up in chords:
        root += voice([(note, 1.5), ("R", 2.5)], wave_fn, root_vol)
        pluck += voice(
            [("R", 1.5), (up, 0.5), ("R", 0.5), (up, 0.5), ("R", 1.0)],
            lambda p: square(p, 0.15),
            pluck_vol,
        )
    return root, pluck

root_e, pluck_e = offbeat(CHORDS_E, triangle, 0.17, 0.11)
melody = voice(MELODY_E, triangle, 0.28)
check = abs(len(melody) - len(root_e))
assert check < RATE * 0.05, f"theme_e voices differ by {check / RATE:.2f}s"
surf = noise(len(melody) / RATE, vol=0.05, lowpass=0.05)
write_music("theme_e", mix(melody, root_e, pluck_e, surf))

# --- theme F: chase the tide, F mixolydian, 4/4 at 152 bpm -------------------
# The busy one, for a board full of crabs: a square lead running straight
# eighths over a walking triangle bass. Flat seventh (Eb) all the way, which
# is what keeps it from sounding like a march.
NOTE.update({"Eb4": 311.13, "Eb5": 622.25, "F5": 698.46, "Eb3": 155.56})
BEAT = 60.0 / 152.0

MELODY_F = [
    ("F4", 0.5), ("G4", 0.5), ("A4", 0.5), ("C5", 0.5),
    ("D5", 0.5), ("C5", 0.5), ("A4", 1),
    ("Bb4", 0.5), ("A4", 0.5), ("G4", 0.5), ("F4", 0.5),
    ("G4", 0.5), ("A4", 0.5), ("F4", 1),
    ("C5", 0.5), ("D5", 0.5), ("Eb5", 0.5), ("F5", 0.5),
    ("Eb5", 0.5), ("D5", 0.5), ("C5", 1),
    ("D5", 0.5), ("C5", 0.5), ("Bb4", 0.5), ("A4", 0.5),
    ("G4", 0.5), ("F4", 0.5), ("F4", 1),
    ("A4", 0.5), ("C5", 0.5), ("F5", 0.5), ("Eb5", 0.5),
    ("C5", 0.5), ("Bb4", 0.5), ("A4", 1),
    ("Bb4", 0.5), ("C5", 0.5), ("D5", 0.5), ("Eb5", 0.5),
    ("D5", 0.5), ("C5", 0.5), ("Bb4", 1),
    ("A4", 0.5), ("G4", 0.5), ("F4", 0.5), ("Eb4", 0.5),
    ("F4", 0.5), ("G4", 0.5), ("A4", 1),
    ("C5", 0.5), ("Bb4", 0.5), ("A4", 0.5), ("G4", 0.5),
    ("F4", 2),
]
BASS_F = [
    ("F3", 1), ("C4", 1), ("F3", 1), ("A3", 1),
    ("Bb3", 1), ("F3", 1), ("Bb3", 1), ("C4", 1),
    ("C4", 1), ("G3", 1), ("C4", 1), ("Eb3", 1),
    ("F3", 1), ("C4", 1), ("F3", 1), ("F3", 1),
    ("F3", 1), ("A3", 1), ("C4", 1), ("A3", 1),
    ("Bb3", 1), ("F3", 1), ("Bb3", 1), ("D3", 1),
    ("Eb3", 1), ("Bb3", 1), ("Eb3", 1), ("C4", 1),
    ("F3", 1), ("C4", 1), ("F3", 1), ("F3", 1),
]
melody = voice(MELODY_F, lambda p: square(p, 0.45), 0.24)
bass = voice(BASS_F, triangle, 0.20)
check = abs(len(melody) - len(bass))
assert check < RATE * 0.05, f"theme_f voices differ by {check / RATE:.2f}s"
surf = noise(len(melody) / RATE, vol=0.04, lowpass=0.06)
write_music("theme_f", mix(melody, bass, surf))

# --- theme G: lantern night, Bb major, 3/4 at 88 bpm ------------------------
# The slow one, for sunset and night rounds: a held triangle melody with a
# thin arpeggio glinting above it, which no other loop has.
NOTE.update({"Bb2": 116.54, "F3": 174.61, "Bb3": 233.08, "Eb4": 311.13,
             "F4": 349.23, "Bb4": 466.16, "D5": 587.33, "F5": 698.46})
BEAT = 60.0 / 88.0

MELODY_G = [
    ("Bb4", 2), ("D5", 1),
    ("F5", 2), ("D5", 1),
    ("C5", 2), ("Bb4", 1),
    ("A4", 3),
    ("G4", 2), ("Bb4", 1),
    ("D5", 2), ("C5", 1),
    ("Bb4", 2), ("A4", 1),
    ("G4", 3),
    ("F4", 2), ("A4", 1),
    ("C5", 2), ("A4", 1),
    ("Bb4", 2), ("G4", 1),
    ("F4", 3),
    ("G4", 1), ("A4", 1), ("Bb4", 1),
    ("D5", 2), ("C5", 1),
    ("Bb4", 3),
    ("Bb4", 3),
]
BASS_G = [
    ("Bb2", 3), ("F3", 3), ("Eb3", 3), ("F3", 3),
    ("G3", 3), ("Bb2", 3), ("Eb3", 3), ("G3", 3),
    ("F3", 3), ("A3", 3), ("Bb2", 3), ("F3", 3),
    ("G3", 3), ("Eb3", 3), ("F3", 3), ("Bb2", 3),
]
# The glint: three rising notes a bar, thin and quiet, one octave up from
# whatever the bass is holding.
GLINT = {"Bb2": "Bb3", "F3": "F4", "Eb3": "Eb4", "G3": "G4", "A3": "A4"}
glint = []
for note, _ in BASS_G:
    up = GLINT[note]
    glint += voice([("R", 1), (up, 0.5), ("R", 0.5), (up, 0.5), ("R", 0.5)],
                   lambda p: square(p, 0.12), 0.07)
melody = voice(MELODY_G, triangle, 0.26)
bass = voice(BASS_G, lambda p: square(p, 0.4), 0.14)
check = abs(len(melody) - len(bass))
assert check < RATE * 0.05, f"theme_g voices differ by {check / RATE:.2f}s"
surf = noise(len(melody) / RATE, vol=0.06, lowpass=0.03)
write_music("theme_g", mix(melody, bass, glint, surf))
