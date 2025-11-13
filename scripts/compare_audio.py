#!/usr/bin/env python3
import wave
import struct
import sys

def compare_wav_files(file1, file2):
    """Compare two WAV files and show statistics about differences"""
    with wave.open(file1, 'rb') as w1, wave.open(file2, 'rb') as w2:
        # Check if formats match
        if w1.getparams() != w2.getparams():
            print(f"WARNING: Files have different formats!")
            print(f"File 1: {w1.getparams()}")
            print(f"File 2: {w2.getparams()}")
            return

        # Read all frames
        frames1 = w1.readframes(w1.getnframes())
        frames2 = w2.readframes(w2.getnframes())

        # Convert to floats (assuming 32-bit float format)
        fmt = f'{len(frames1)//4}f'
        samples1 = struct.unpack(fmt, frames1)
        samples2 = struct.unpack(fmt, frames2)

        # Calculate differences
        diffs = [abs(s1 - s2) for s1, s2 in zip(samples1, samples2)]

        # Statistics
        max_diff = max(diffs)
        avg_diff = sum(diffs) / len(diffs)
        num_different = sum(1 for d in diffs if d > 0.0001)  # threshold for "different"
        percent_different = (num_different / len(diffs)) * 100

        # Sample values
        print(f"Total samples: {len(samples1)}")
        print(f"Samples that differ: {num_different} ({percent_different:.2f}%)")
        print(f"Maximum difference: {max_diff:.6f}")
        print(f"Average difference: {avg_diff:.6f}")
        print(f"\nFirst 10 samples from each file:")
        print(f"File 1: {[f'{s:.4f}' for s in samples1[:10]]}")
        print(f"File 2: {[f'{s:.4f}' for s in samples2[:10]]}")
        print(f"Diffs:  {[f'{d:.4f}' for d in diffs[:10]]}")

if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("Usage: compare_audio.py <file1.wav> <file2.wav>")
        sys.exit(1)

    compare_wav_files(sys.argv[1], sys.argv[2])
