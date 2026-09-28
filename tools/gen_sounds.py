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


def tone(freqs, dur, wave_fn=square, vol=0.5, bend=1.0, vibrato=0.0,
         attack=0.01, release=0.25):
    """One note or a slide across `freqs` (start->end), with optional vibrato.
    `attack` and `release` shape the envelope as fractions of the note."""
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
        out.append(wave_fn(phase) * vol * env(i, n, attack, release))
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

# Signpost evicted at the cap: a soft descending scuff, the oldest post
# pulled out of the sand to make room for the fourth. Deliberately quiet
# and short - the placement it paid for sounds in the same frame, from the
# other end of the beach, and this one sits underneath it.
write("evict", mix(
    noise(0.20, vol=0.15, lowpass=0.30),
    tone((660, 300), 0.20, triangle, vol=0.22),
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

# --- the soft three: H, I and J ---------------------------------------------
# Added after players called the music repetitive and, some of them,
# annoying. B to G are square and triangle leads sitting in the same
# register as the effects, one phrase each, never resting. These three are
# the answer to that: sine-based voices with softer note edges, melodies
# pitched lower, and 24 bars in an A-B-A shape whose middle thins out, so
# each track has somewhere to breathe. About twice as long as the others.

def hz(name):
    """Equal-tempered frequency of a note name like "Eb4" or "F#3"."""
    steps = {"C": -9, "D": -7, "E": -5, "F": -4, "G": -2, "A": 0, "B": 2}
    semis = steps[name[0]] + name.count("#") - name[1:].count("b")
    octave = int(name.lstrip("ABCDEFG#b"))
    return 440.0 * 2 ** ((semis + 12 * (octave - 4)) / 12)


def mellow(phase):
    """A sine with a touch of its octave: round, like a marimba bar."""
    return 0.85 * math.sin(2 * math.pi * phase) + 0.15 * math.sin(4 * math.pi * phase)


def sine(phase):
    return math.sin(2 * math.pi * phase)


def play(seq, beat, wave_fn, vol, **shape):
    """A line of (note, beats) at `beat` seconds a beat; "R" rests."""
    out = []
    for name, beats in seq:
        dur = beats * beat
        if name == "R":
            out += [0.0] * int(dur * RATE)
        else:
            out += tone(hz(name), dur, wave_fn, vol=vol, **shape)
    return out


def same_length(name, *voices):
    longest = max(len(v) for v in voices)
    for v in voices:
        gap = longest - len(v)
        assert gap < RATE * 0.05, f"{name} voices differ by {gap / RATE:.2f}s"


# --- theme H: rock pools, C major, 4/4 at 100 bpm ----------------------------
# A marimba melody over a two-note bass. The B section drops the melody an
# octave's worth of weight and leaves half of every bar empty.
BEAT_H = 60.0 / 100.0
A_H = [
    ("E4", 1), ("G4", 1), ("C5", 1.5), ("B4", 0.5),
    ("A4", 2), ("E4", 1), ("R", 1),
    ("F4", 1), ("A4", 1), ("C5", 1), ("A4", 1),
    ("G4", 3), ("R", 1),
    ("E4", 1), ("G4", 1), ("C5", 1), ("D5", 1),
    ("E5", 1.5), ("D5", 0.5), ("C5", 1), ("A4", 1),
    ("F4", 1), ("A4", 1), ("D5", 1), ("C5", 1),
]
MELODY_H = A_H + [
    ("B4", 2), ("G4", 1), ("R", 1),
    ("R", 2), ("C5", 1), ("B4", 1),
    ("G4", 3), ("R", 1),
    ("R", 2), ("A4", 1), ("G4", 1),
    ("E4", 3), ("R", 1),
    ("R", 1), ("D4", 1), ("F4", 1), ("A4", 1),
    ("C5", 2), ("B4", 1), ("A4", 1),
    ("A4", 1), ("G4", 1), ("F4", 1), ("E4", 1),
    ("D4", 3), ("R", 1),
] + A_H + [("C5", 3), ("R", 1)]
# One chord a bar, as (root, fifth).
CHORDS_H = [
    ("C3", "G3"), ("A2", "E3"), ("F2", "C3"), ("G2", "D3"),
    ("C3", "G3"), ("A2", "E3"), ("D3", "A3"), ("G2", "D3"),
    ("A2", "E3"), ("E3", "B3"), ("F2", "C3"), ("C3", "G3"),
    ("D3", "A3"), ("A2", "E3"), ("F2", "C3"), ("G2", "D3"),
    ("C3", "G3"), ("A2", "E3"), ("F2", "C3"), ("G2", "D3"),
    ("C3", "G3"), ("A2", "E3"), ("D3", "A3"), ("C3", "G3"),
]
BASS_H = [step for root, fifth in CHORDS_H
          for step in ((root, 1.5), ("R", 0.5), (fifth, 1.5), ("R", 0.5))]
melody = play(MELODY_H, BEAT_H, mellow, 0.30, release=0.15)
bass = play(BASS_H, BEAT_H, triangle, 0.17, attack=0.03, release=0.4)
same_length("theme_h", melody, bass)
surf = noise(len(melody) / RATE, vol=0.05, lowpass=0.04)
write_music("theme_h", mix(melody, bass, surf))

# --- theme I: driftwood, D dorian, 3/4 at 108 bpm ----------------------------
# A whistled tune: a sine with a slow swell and a little vibrato, over a
# plucked bass. The B natural of the dorian mode is what gives it the
# folk-song lilt none of the others has.
BEAT_I = 60.0 / 108.0
A_I = [
    ("D4", 2), ("F4", 1),
    ("E4", 1), ("G4", 2),
    ("A4", 2), ("G4", 1),
    ("E4", 3),
    ("F4", 1), ("A4", 1), ("C5", 1),
    ("B4", 2), ("G4", 1),
    ("D5", 1.5), ("C5", 0.5), ("B4", 1),
]
MELODY_I = A_I + [
    ("A4", 3),
    ("B4", 1), ("A4", 1), ("G4", 1),
    ("F4", 3),
    ("E4", 1), ("F4", 1), ("G4", 1),
    ("A4", 3),
    ("R", 1), ("C5", 1), ("A4", 1),
    ("B4", 1), ("G4", 2),
    ("E4", 1), ("G4", 1), ("E4", 1),
    ("D4", 3),
] + A_I + [("D4", 3)]
# The chord under each bar, as (root, fifth).
ROOTS_I = {"Dm": ("D3", "A3"), "C": ("C3", "G3"), "F": ("F2", "C3"),
           "G": ("G2", "D3")}
CHORDS_I = ["Dm", "C", "Dm", "C", "F", "C", "G", "Dm",
            "G", "Dm", "C", "Dm", "F", "G", "C", "Dm",
            "Dm", "C", "Dm", "C", "F", "C", "G", "Dm"]
BASS_I = [step for chord in CHORDS_I
          for step in ((ROOTS_I[chord][0], 1), ("R", 1),
                       (ROOTS_I[chord][1], 0.5), ("R", 0.5))]
melody = play(MELODY_I, BEAT_I, sine, 0.26, vibrato=0.004,
              attack=0.12, release=0.6)
bass = play(BASS_I, BEAT_I, triangle, 0.18, release=0.2)
same_length("theme_i", melody, bass)
surf = noise(len(melody) / RATE, vol=0.05, lowpass=0.03)
write_music("theme_i", mix(melody, bass, surf))

# --- theme J: low tide shuffle, Eb major, 4/4 at 112 bpm ---------------------
# Led from the bottom: a walking bass and soft chord stabs on 2 and 4 carry
# the first eight bars alone, the swung melody comes in for eight, and the
# last eight trade short calls with the groove.
BEAT_J = 60.0 / 112.0
S, L = 1 / 3, 2 / 3  # a swung pair of eighths: long, then short
MELODY_J = [("R", 32)] + [
    ("Bb4", 1), ("G4", L), ("Bb4", S), ("C5", 1), ("Bb4", 1),
    ("G4", 2), ("R", 1), ("Eb4", 1),
    ("C5", L), ("Bb4", S), ("Ab4", L), ("G4", S), ("Ab4", 1), ("C5", 1),
    ("Bb4", 3), ("R", 1),
    ("Eb5", 1), ("D5", L), ("C5", S), ("Bb4", 1), ("G4", 1),
    ("C5", 2), ("G4", 1), ("R", 1),
    ("Ab4", 1), ("C5", 1), ("Bb4", L), ("Ab4", S), ("G4", 1),
    ("F4", 2), ("R", 2),
    ("G4", L), ("Bb4", S), ("Eb5", 1), ("R", 2),
    ("R", 4),
    ("Ab4", L), ("C5", S), ("Eb5", 1), ("R", 2),
    ("R", 2), ("D5", 1), ("Bb4", 1),
    ("G4", L), ("Bb4", S), ("Eb5", 1), ("R", 2),
    ("R", 2), ("Eb5", 1), ("C5", 1),
    ("Bb4", 1), ("Ab4", 1), ("G4", 1), ("F4", 1),
    ("Eb4", 3), ("R", 1),
]
WALK_J = {
    "Eb": [("Eb3", 1), ("G3", 1), ("Bb3", 1), ("G3", 1)],
    "Cm": [("C3", 1), ("Eb3", 1), ("G3", 1), ("Eb3", 1)],
    "Ab": [("Ab2", 1), ("C3", 1), ("Eb3", 1), ("C3", 1)],
    "Bb": [("Bb2", 1), ("D3", 1), ("F3", 1), ("D3", 1)],
}
# The two chord tones each stab plays.
STAB_J = {"Eb": ("G4", "Bb4"), "Cm": ("G4", "C5"), "Ab": ("Ab4", "C5"),
          "Bb": ("F4", "Bb4")}
CHORDS_J = ["Eb", "Cm", "Ab", "Bb"] * 6
bass = play([step for chord in CHORDS_J[:-1] for step in WALK_J[chord]]
            + [("Eb3", 3), ("R", 1)], BEAT_J, triangle, 0.20, release=0.35)
stabs = []
for i, chord in enumerate(CHORDS_J):
    lo, hi = STAB_J["Eb" if i == len(CHORDS_J) - 1 else chord]
    bar = [("R", 1), (None, 0.3), ("R", 1.7), (None, 0.3), ("R", 0.7)]
    for name, beats in bar:
        if name == "R":
            stabs += [0.0] * int(beats * BEAT_J * RATE)
        else:
            dur = beats * BEAT_J
            stabs += mix(tone(hz(lo), dur, mellow, vol=0.07, release=0.3),
                         tone(hz(hi), dur, mellow, vol=0.07, release=0.3))
melody = play(MELODY_J, BEAT_J, mellow, 0.28, attack=0.02, release=0.3)
same_length("theme_j", melody, bass, stabs)
surf = noise(len(melody) / RATE, vol=0.05, lowpass=0.05)
write_music("theme_j", mix(melody, bass, stabs, surf))

# --- the club three: K, L and M ---------------------------------------------
# Electronic dance tracks: drums, a bass that pumps against the kick, and a
# synth hook. They need what the chiptune loops never did - a drum kit, a
# filter and a sidechain - so they are built on a sequencer grid of
# sixteenth notes and summed into one buffer rather than voice by voice.
import random

_hiss = random.Random(0x5EA5)  # seeded, so every run writes the same files


def saw(phase):
    return 2.0 * (phase % 1.0) - 1.0


def lowpass(samples, cutoff):
    """One-pole low-pass at `cutoff` Hz, or a cutoff per sample (a sweep)."""
    out, y = [], 0.0
    sweep = callable(cutoff)
    a = 1.0 - math.exp(-2 * math.pi * (0 if sweep else cutoff) / RATE)
    for i, x in enumerate(samples):
        if sweep:
            a = 1.0 - math.exp(-2 * math.pi * cutoff(i) / RATE)
        y += a * (x - y)
        out.append(y)
    return out


def highpass(samples, cutoff):
    return [x - l for x, l in zip(samples, lowpass(samples, cutoff))]


def white(n):
    return [_hiss.uniform(-1.0, 1.0) for _ in range(n)]


def kick():
    """A sine dropping from 160 Hz to 45 Hz in a few milliseconds."""
    n, out, phase = int(0.35 * RATE), [], 0.0
    for i in range(n):
        t = i / RATE
        phase += (45 + 115 * math.exp(-t * 30)) / RATE
        out.append(math.sin(2 * math.pi * phase) * math.exp(-t * 7))
    return out


def snare():
    n = int(0.2 * RATE)
    rattle = highpass(white(n), 1500)
    return [0.5 * math.sin(2 * math.pi * 185 * i / RATE) * math.exp(-i / RATE * 30)
            + rattle[i] * math.exp(-i / RATE * 22) for i in range(n)]


def clap():
    """Three hand-slaps a hair apart, then the room."""
    n = int(0.25 * RATE)
    hiss = highpass(lowpass(white(n), 5000), 800)
    out = []
    for i, h in enumerate(hiss):
        t = i / RATE
        slaps = sum(math.exp(-(t - k * 0.01) * 150) for k in range(3) if t >= k * 0.01)
        tail = 0.4 * math.exp(-(t - 0.03) * 18) if t >= 0.03 else 0.0
        out.append(h * (slaps + tail))
    return out


def hat(open_=False):
    n = int((0.25 if open_ else 0.06) * RATE)
    decay = 15 if open_ else 70
    return [h * math.exp(-i / RATE * decay) for i, h in enumerate(highpass(white(n), 7000))]


def synth(freq, dur, wave_fn, vol, attack=0.005, release=0.3, cutoff=None,
          voices=1, detune=0.012):
    """One synth note: `voices` detuned copies (a supersaw at 3), filtered."""
    n = int(dur * RATE)
    spread = [1.0 + detune * (k - (voices - 1) / 2) for k in range(voices)]
    phases = [k / voices for k in range(voices)]
    out = []
    for i in range(n):
        s = 0.0
        for k, ratio in enumerate(spread):
            phases[k] += freq * ratio / RATE
            s += wave_fn(phases[k])
        out.append(s / voices * vol * env(i, n, attack, release))
    return lowpass(out, cutoff) if cutoff else out


def place(buf, samples, at, gain=1.0):
    start = int(at * RATE)
    for i, s in enumerate(samples[: max(0, len(buf) - start)]):
        buf[start + i] += s * gain


def sidechain(buf, hits, depth=0.7, recover=0.2):
    """Duck `buf` under every hit, the pump that makes the bass breathe."""
    for at in hits:
        start = int(at * RATE)
        n = int(recover * RATE)
        for i in range(min(n, len(buf) - start)):
            buf[start + i] *= 1.0 - depth * (1.0 - i / n) ** 2


def master(buf, loudness_db=-22.5):
    """Level with the other tracks (by RMS), then round off any peaks."""
    rms = math.sqrt(sum(x * x for x in buf) / len(buf))
    gain = 10 ** (loudness_db / 20) / rms
    out = []
    for x in buf:
        x *= gain
        if abs(x) > 0.8:
            x = math.copysign(0.8 + 0.2 * math.tanh((abs(x) - 0.8) / 0.2), x)
        out.append(x)
    return out


KICK, SNARE, CLAP, HAT, OPEN_HAT = kick(), snare(), clap(), hat(), hat(open_=True)


class Track:
    """A buffer `bars` long on a sixteenth-note grid, with drums and synth
    buses kept apart so only the synths pump against the kick."""

    def __init__(self, bpm, bars):
        self.step = 60.0 / bpm / 4
        n = int(bars * 16 * self.step * RATE)
        self.drums, self.synths = [0.0] * n, [0.0] * n
        self.kicks = []

    def at(self, bar, step):
        return (bar * 16 + step) * self.step

    def hit(self, sample, bar, steps, gain):
        for s in steps:
            if sample is KICK:
                self.kicks.append(self.at(bar, s))
            place(self.drums, sample, self.at(bar, s), gain)

    def note(self, samples, bar, step, gain=1.0):
        place(self.synths, samples, self.at(bar, step), gain)

    def mix(self, depth=0.7, recover=0.2):
        sidechain(self.synths, self.kicks, depth, recover)
        return master([d + s for d, s in zip(self.drums, self.synths)])


FOUR_ON_THE_FLOOR = [0, 4, 8, 12]
OFFBEATS = [2, 6, 10, 14]
TRIADS = {
    "Am": ("A3", "C4", "E4"), "F": ("F3", "A3", "C4"), "C": ("C4", "E4", "G4"),
    "G": ("G3", "B3", "D4"), "Fm": ("F3", "Ab3", "C4"), "Db": ("Db4", "F4", "Ab4"),
    "Ab": ("Ab3", "C4", "Eb4"), "Eb": ("Eb4", "G4", "Bb4"), "D": ("D4", "F#4", "A4"),
    "A": ("A3", "C#4", "E4"), "Bm": ("B3", "D4", "F#4"),
}


def down(name, octaves):
    """The same note `octaves` lower, for a bass under a triad."""
    return name.rstrip("0123456789") + str(int(name.lstrip("ABCDEFG#b")) - octaves)


def hook(track, bar, line, wave_fn, vol, cutoff, **shape):
    """A melody as (start step, length in steps, note) from `bar` on."""
    for start, length, name in line:
        track.note(synth(hz(name), length * track.step, wave_fn, vol,
                         cutoff=cutoff, **shape), bar, start)


# --- theme K: neon tide, A minor house at 124 bpm ----------------------------
# Four on the floor, an off-beat bass, and a plucked arpeggio that opens up
# as the track does. Intro, sixteen bars of groove with a hook over the
# second half, and a breakdown of pads to hand back to the quiet.
k = Track(124, 32)
CHORDS_K = ["Am", "F", "C", "G"] * 8
for bar, chord in enumerate(CHORDS_K):
    root, third, fifth = TRIADS[chord]
    groove = 8 <= bar < 24
    if 4 <= bar < 24:
        k.hit(KICK, bar, FOUR_ON_THE_FLOOR, 0.9)
    if bar < 24:
        k.hit(HAT, bar, range(16), 0.10)
    if groove:
        k.hit(CLAP, bar, [4, 12], 0.45)
        k.hit(OPEN_HAT, bar, OFFBEATS, 0.12)
        for s in OFFBEATS:
            k.note(synth(hz(down(root, 1)), 1.6 * k.step, saw, 0.30,
                         release=0.4, cutoff=700), bar, s)
    # The arpeggio, brighter in the groove than either side of it.
    cutoff = 2400 if groove else 1000
    arp = [root, third, fifth, down(root, -1), fifth, third]
    for s in range(16):
        k.note(synth(hz(arp[s % len(arp)]), k.step, lambda p: square(p, 0.25),
                     0.20, release=0.2, cutoff=cutoff), bar, s)
    if bar >= 24:
        for name in (root, third, fifth):
            k.note(synth(hz(name), 16 * k.step, saw, 0.16, attack=0.15,
                         release=3.0, cutoff=1200, voices=3), bar, 0)
HOOK_K = [(0, 3, "E5"), (3, 3, "D5"), (6, 2, "C5"), (8, 4, "A4"), (14, 2, "C5"),
          (16, 3, "D5"), (19, 3, "C5"), (22, 2, "B4"), (24, 6, "G4")]
for bar in range(16, 24, 2):
    hook(k, bar, HOOK_K, lambda p: square(p, 0.5), 0.13, 1800, release=0.5)
write_music("theme_k", k.mix())

# --- theme L: reef drop, F minor at 128 bpm ----------------------------------
# The one with a drop: pads, a snare roll that tightens over four bars under
# a rising sweep, a beat of silence, then kick, rolling bass and pumping
# supersaw stabs, with the hook on the second pass.
l = Track(128, 32)
CHORDS_L = ["Fm", "Db", "Ab", "Eb"] * 8
for bar, chord in enumerate(CHORDS_L):
    root, third, fifth = TRIADS[chord]
    if bar < 8:
        # Pads open up from the intro through the build.
        cutoff = 700 + 300 * bar
        for name in (root, third, fifth):
            l.note(synth(hz(name), (12 if bar == 7 else 16) * l.step, saw, 0.16,
                         attack=0.1, release=3.0, cutoff=cutoff, voices=3), bar, 0)
        l.hit(HAT, bar, range(0, 16, 2), 0.09)
    if 4 <= bar < 8:
        # The roll: quarters, eighths, then sixteenths, louder as it goes,
        # stopping a beat short so the drop lands on silence.
        every = {4: 4, 5: 2}.get(bar, 1)
        steps = range(0, 12 if bar == 7 else 16, every)
        l.hit(SNARE, bar, steps, 0.15 + 0.07 * (bar - 4))
    if 8 <= bar < 28:
        l.hit(KICK, bar, FOUR_ON_THE_FLOOR, 0.9)
        l.hit(HAT, bar, range(16), 0.08)
    if 8 <= bar < 24:
        l.hit(CLAP, bar, [4, 12], 0.45)
        l.hit(OPEN_HAT, bar, OFFBEATS, 0.11)
        for s in OFFBEATS:
            for name in (root, third, fifth):
                l.note(synth(hz(name), 1.5 * l.step, saw, 0.09, release=0.5,
                             cutoff=2600, voices=3), bar, s)
    if 8 <= bar < 28:
        # Rolling bass: every sixteenth the kick does not own, darker in
        # the outro.
        cutoff = 900 if bar < 24 else 400
        for s in range(16):
            if s % 4:
                l.note(synth(hz(down(root, 1)), 0.9 * l.step, saw, 0.26,
                             release=0.5, cutoff=cutoff), bar, s)
    if bar >= 28:
        fade = (32 - bar) / 5
        for name in (root, third, fifth):
            l.note(synth(hz(name), 16 * l.step, saw, 0.16 * fade, attack=0.1,
                         release=3.0, cutoff=900, voices=3), bar, 0)
# The build's sweep: noise opening from 300 Hz to 8 kHz over four bars.
span = int(16 * 4 * l.step * RATE)
sweep = lowpass([x * 0.25 * i / span for i, x in enumerate(white(span))],
                lambda i: 300 + 7700 * (i / span) ** 2)
place(l.drums, sweep, l.at(4, 0))
HOOK_L = [
    (0, 3, "C5"), (3, 3, "Ab4"), (6, 2, "F4"), (8, 2, "G4"), (10, 2, "Ab4"), (12, 4, "C5"),
    (16, 3, "Db5"), (19, 3, "Ab4"), (22, 2, "F4"), (24, 2, "Ab4"), (26, 2, "Db5"), (28, 4, "C5"),
    (32, 3, "C5"), (35, 3, "Eb5"), (38, 2, "C5"), (40, 4, "Ab4"), (44, 4, "Eb4"),
    (48, 3, "G4"), (51, 3, "Bb4"), (54, 2, "Eb5"), (56, 4, "Bb4"), (60, 4, "G4"),
]
for bar in (16, 20):
    hook(l, bar, HOOK_L, lambda p: square(p, 0.3), 0.12, 2200, release=0.4)
write_music("theme_l", l.mix(depth=0.8))

# --- theme M: undertow, D major future bass at 150 bpm, half-time ------------
# Kick and snare at half the tempo, so it sways rather than drives; wide
# supersaw chords chopped 3-3-2 and pumping hard; a sine sub underneath and
# a soft vibrato lead on top.
m = Track(150, 32)
CHORDS_M = ["D", "A", "Bm", "G"] * 8
CHOPS = [(0, 3), (3, 3), (6, 4), (10, 3), (13, 3)]
for bar, chord in enumerate(CHORDS_M):
    root, third, fifth = TRIADS[chord]
    main = 8 <= bar < 24
    if main:
        m.hit(KICK, bar, [0, 10], 0.9)
        m.hit(SNARE, bar, [8], 0.5)
        m.hit(CLAP, bar, [8], 0.25)
        m.hit(HAT, bar, [0, 2, 4, 6, 8, 10, 12, 14, 15], 0.10)
        for start, length in CHOPS:
            for name in (root, third, fifth, down(root, -1)):
                m.note(synth(hz(name), length * m.step, saw, 0.06, attack=0.05,
                             release=0.8, cutoff=3000, voices=3), bar, start)
        m.note(synth(hz(down(root, 2)), 16 * m.step, sine, 0.30, attack=0.02,
                     release=3.0), bar, 0)
    else:
        # Either side of the drop, the chords held soft and whole.
        fade = 1.0 if bar < 8 else (32 - bar) / 8
        if bar >= 24:
            m.hit(KICK, bar, [0], 0.6 * fade)
        m.hit(HAT, bar, range(0, 16, 4), 0.08 * fade)
        for name in (root, third, fifth):
            m.note(synth(hz(name), 16 * m.step, saw, 0.16 * fade, attack=0.2,
                         release=3.0, cutoff=1400, voices=3), bar, 0)
# The snare pumps the chords too: that double breath is the style.
m.kicks += [m.at(bar, 8) for bar in range(8, 24)]
HOOK_M = [
    (0, 2, "F#4"), (2, 2, "A4"), (4, 4, "D5"), (8, 2, "C#5"), (10, 6, "A4"),
    (16, 2, "E4"), (18, 2, "A4"), (20, 4, "C#5"), (24, 2, "B4"), (26, 6, "A4"),
    (32, 2, "F#4"), (34, 2, "B4"), (36, 4, "D5"), (40, 2, "C#5"), (42, 6, "B4"),
    (48, 2, "G4"), (50, 2, "B4"), (52, 4, "D5"), (56, 4, "E5"), (60, 4, "D5"),
]
for bar in (12, 16, 20):
    hook(m, bar, HOOK_M, sine, 0.22, None, attack=0.05, release=0.6)
write_music("theme_m", m.mix(depth=0.85, recover=0.3))

# --- the second club three: N, O and P ---------------------------------------
# Players loved K, L and M, so versus rounds got a set of their own: these
# three join them there, and the calmer tracks keep the menus and puzzles.
TRIADS.update({"Gm": ("G3", "Bb3", "D4"), "Bb": ("Bb3", "D4", "F4"),
               "Em": ("E3", "G3", "B3"), "Cm": ("C4", "Eb4", "G4")})

# --- theme N: sandbar, G minor trance at 138 bpm -----------------------------
# The trance gate: supersaw chords chopped into a sixteenth-note stutter,
# over a bass on every sixteenth the kick leaves free. A breakdown with a
# snare roll halfway, then the lead arpeggio leaps an octave over it all.
n_ = Track(138, 32)
CHORDS_N = ["Gm", "Eb", "Bb", "F"] * 8
GATE = [0, 2, 3, 5, 6, 8, 10, 11, 13, 14]
for bar, chord in enumerate(CHORDS_N):
    root, third, fifth = TRIADS[chord]
    drums = bar < 12 or 16 <= bar < 32
    breakdown = 12 <= bar < 16
    if drums:
        fade = 1.0 if bar < 28 else (32 - bar) / 5
        n_.hit(KICK, bar, FOUR_ON_THE_FLOOR, 0.9 * fade)
        n_.hit(HAT, bar, range(16), 0.08 * fade)
        n_.hit(OPEN_HAT, bar, OFFBEATS, 0.10 * fade)
        for s in range(16):
            if s % 4:
                n_.note(synth(hz(down(root, 1)), 0.9 * n_.step, saw, 0.24 * fade,
                              release=0.5, cutoff=800), bar, s)
    if 4 <= bar < 12 or 16 <= bar < 28:
        n_.hit(CLAP, bar, [4, 12], 0.40)
        for s in GATE:
            for name in (root, third, fifth):
                n_.note(synth(hz(name), n_.step, saw, 0.07, release=0.6,
                              cutoff=2800, voices=3), bar, s)
    if breakdown:
        for name in (root, third, fifth):
            n_.note(synth(hz(name), 16 * n_.step, saw, 0.15, attack=0.1,
                          release=3.0, cutoff=1600, voices=3), bar, 0)
        if bar >= 14:
            every = 2 if bar == 14 else 1
            n_.hit(SNARE, bar, range(0, 12 if bar == 15 else 16, every),
                   0.18 + 0.08 * (bar - 14))
    if 16 <= bar < 28:
        # The lead: an arpeggio up the triad and over the octave.
        arp = [root, third, fifth, down(root, -1), down(third, -1), down(root, -1),
               fifth, third]
        for s in range(0, 16, 2):
            n_.note(synth(hz(down(arp[s // 2], -1)), 2 * n_.step,
                          lambda p: square(p, 0.3), 0.10, release=0.3,
                          cutoff=2600), bar, s)
write_music("theme_n", n_.mix(depth=0.75))

# --- theme O: riptide, E minor drum and bass at 174 bpm ----------------------
# The fastest in the game: a two-step break (kick on 1 and the "and" of 3,
# snare on 2 and 4) with ghost snares, under a reese bass - two saws
# slightly out of tune, beating against each other - and a wide pad.
o = Track(174, 40)
CHORDS_O = ["Em", "C", "G", "D"] * 10
for bar, chord in enumerate(CHORDS_O):
    root, third, fifth = TRIADS[chord]
    full = 8 <= bar < 32
    fade = 1.0 if bar < 32 else (40 - bar) / 9
    o.hit(HAT, bar, range(0, 16, 2), 0.09 * fade)
    if full or bar >= 32:
        o.hit(KICK, bar, [0, 10], 0.85 * fade)
        o.hit(SNARE, bar, [4, 12], 0.55 * fade)
    if full:
        o.hit(SNARE, bar, [7, 15] if bar % 2 else [9], 0.12)  # ghosts
        o.hit(HAT, bar, [3, 11], 0.06)
        o.note(synth(hz(down(root, 2)), 16 * o.step, saw, 0.30, attack=0.01,
                     release=2.0, cutoff=450, voices=2, detune=0.02), bar, 0)
    for name in (root, third, fifth):
        # Carrying the intro alone, the pad has to be heard over nothing.
        o.note(synth(hz(name), 16 * o.step, saw, (0.22 if bar < 8 else 0.10) * fade, attack=0.2,
                     release=3.0, cutoff=1300, voices=3), bar, 0)
    if 16 <= bar < 32 and bar % 2 == 0:
        # A stab answer every other bar, up an octave.
        for s, name in ((0, fifth), (3, third), (6, root), (10, third)):
            o.note(synth(hz(down(name, -1)), 2 * o.step, lambda p: square(p, 0.4),
                         0.10, release=0.3, cutoff=2400), bar, s)
write_music("theme_o", o.mix(depth=0.4, recover=0.15))

# --- theme P: crab walk, C minor electro wobble at 126 bpm -------------------
# The wobble: a square bass whose filter sweeps open and shut on every
# eighth note, under four-on-the-floor and a brass-like stab hook.
p_ = Track(126, 32)
CHORDS_P = ["Cm", "Ab", "Eb", "Bb"] * 8
for bar, chord in enumerate(CHORDS_P):
    root, third, fifth = TRIADS[chord]
    drop = 8 <= bar < 24
    if bar < 28:
        fade = 1.0 if bar < 24 else 0.7
        p_.hit(KICK, bar, FOUR_ON_THE_FLOOR, 0.9 * fade)
        p_.hit(HAT, bar, range(0, 16, 2), 0.10 * fade)
    if 4 <= bar < 24:
        p_.hit(CLAP, bar, [4, 12], 0.45)
    if 4 <= bar < 8 or bar >= 24:
        fade = (32 - bar) / 4 if bar >= 28 else 1.0
        for name in (root, third, fifth):
            p_.note(synth(hz(name), 16 * p_.step, saw, 0.18 * fade, attack=0.1,
                          release=3.0, cutoff=1200, voices=3), bar, 0)
    if drop:
        rate = 1 / (2 * p_.step)  # one wobble an eighth note
        wobble = lambda i, rate=rate: 250 + 1900 * (0.5 - 0.5 * math.cos(2 * math.pi * i / RATE * rate))
        p_.note(synth(hz(down(root, 2)), 16 * p_.step, lambda q: square(q, 0.5), 0.32,
                      attack=0.01, release=3.0, cutoff=wobble), bar, 0)
        p_.hit(OPEN_HAT, bar, OFFBEATS, 0.10)
HOOK_P = [
    (0, 2, "G4"), (3, 2, "G4"), (6, 2, "Eb4"), (8, 2, "F4"), (10, 4, "G4"),
    (16, 2, "Ab4"), (19, 2, "Ab4"), (22, 2, "G4"), (24, 2, "Eb4"), (26, 4, "C4"),
    (32, 2, "Bb4"), (35, 2, "Bb4"), (38, 2, "G4"), (40, 2, "Bb4"), (42, 4, "C5"),
    (48, 2, "D5"), (51, 2, "C5"), (54, 2, "Bb4"), (56, 4, "F4"), (60, 4, "D4"),
]
for bar in (12, 16, 20):
    hook(p_, bar, HOOK_P, saw, 0.13, 1600, voices=2, release=0.5)
write_music("theme_p", p_.mix(depth=0.75))
