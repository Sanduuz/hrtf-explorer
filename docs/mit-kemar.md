# MIT compact KEMAR dataset notes

## Selection

The project uses Bill Gardner and Keith Martin's compact MIT Media Lab KEMAR measurements:

- Project archive: <https://sound.media.mit.edu/resources/KEMAR.html>
- Technical documentation: <https://sound.media.mit.edu/resources/KEMAR/hrtfdoc.txt>
- FAQ and format clarifications: <https://sound.media.mit.edu/resources/KEMAR/KEMAR-FAQ.txt>
- Selected download: <https://sound.media.mit.edu/resources/KEMAR/compact.zip>

The compact set was selected because it was prepared specifically for 3D audio synthesis, contains short stereo responses suitable for direct MVP convolution, retains interaural delay, and has well-documented directions and processing.

## Verified metadata

| Property | Value |
| --- | --- |
| Measurement system | KEMAR DB-4004 dummy head and torso |
| Measurement distance | 1.4 m |
| Environment | MIT anechoic chamber |
| Sample rate | 44,100 Hz |
| Runtime HRIR length | 128 samples per ear |
| Measured elevation range | -40° through +90° |
| Complete direction count | 710 |
| Original compact encoding | Headerless, big-endian signed 16-bit stereo PCM; left/right interleaved |
| Selected ZIP encoding | RIFF/WAVE, little-endian signed 16-bit stereo PCM; left/right interleaved |
| Source files in ZIP | 368, covering azimuths 0° through 180° |
| Runtime sample encoding | Little-endian IEEE-754 `f32`, scaled from PCM by `sample / 32768.0` |

The selected ZIP was added after the original documentation and contains WAV conversions rather than the original headerless `.dat` files. Inspection confirms PCM format 1, two channels, 44,100 Hz, 16 bits, and 128 frames. Every source WAV is 556 bytes: a 44-byte RIFF/WAVE header followed by 512 bytes of audio.

Archive checksums recorded during conversion:

```text
compact.zip   0bcd69f8e8760cf8eacff4a89594ceca7d44fb94f28ecf0bf19970003bbfb3e0
compact.tar.Z 2548d9e78db5e74c4a39aaee2061c247bafed2f9d0da070d51f7a74a35fe4797
```

Both official archives contain 368 compact source positions.

## Coordinates and symmetry

The source convention matches the project's canonical convention:

- azimuth 0° is in front;
- azimuth 90° is to the listener's right;
- elevation +90° is directly above.

No axis remapping is necessary. Source azimuths greater than 180° are represented by the project's equivalent negative canonical azimuth after conversion to a direction vector.

The compact data was derived symmetrically from the KEMAR's left-ear measurements. A compact file at azimuth `a` contains the left response from the original left-ear measurement at `a` and the right response from the left-ear measurement at `360-a`. Consequently the official compact archives only store 0°–180°. For each non-median source file, `hrtf-convert` creates the missing direction at `360-a` and swaps its ear channels. Median-plane positions at 0° and 180° are not duplicated. This expands 368 source pairs to the documented 710-direction sphere.

## Processing already present in the compact set

The 128-sample responses were derived from the 512-sample measurements by convolving with a minimum-phase inverse of the measurement loudspeaker response and retaining 128 samples beginning at sample index 26. The data still includes interaural time differences. Direct interpolation may smear those arrival-time differences; this remains a documented MVP limitation.

The compact set contains an ear-canal response and is not individualized for the listener. Perceived localization varies between listeners and headphones.

## License and attribution

The archive states that the data is Copyright 1994 MIT Media Laboratory, is provided without usage restrictions, and asks that the authors be cited in research or commercial applications:

> Bill Gardner and Keith Martin, MIT Media Lab, “HRTF Measurements of a KEMAR Dummy-Head Microphone,” 1994.

## Reproducing the conversion

Download and extract `compact.zip`, then run:

```bash
cargo run -p hrtf-convert -- /path/to/extracted/compact frontend/assets/mit-kemar.bhrtf
```

For a small validation subset:

```bash
cargo run -p hrtf-convert -- /path/to/extracted/compact /tmp/mit-kemar-subset.bhrtf --limit 4
```

The converter validates filenames, angle ranges, RIFF/WAVE structure, PCM metadata, sample count, duplicate positions, expected full-source count, expected expanded count, dataset invariants, and a serialize/deserialize round trip.

