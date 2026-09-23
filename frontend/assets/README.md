# MIT KEMAR runtime dataset

`mit-kemar.bhrtf` is generated from the MIT Media Lab compact KEMAR HRTF measurements. It contains 710 stereo measurement directions, sampled at 44,100 Hz with 128 samples per ear.

Source archive: <https://sound.media.mit.edu/resources/KEMAR/compact.zip>

Source archive SHA-256:

```text
0bcd69f8e8760cf8eacff4a89594ceca7d44fb94f28ecf0bf19970003bbfb3e0
```

Generated runtime file SHA-256:

```text
c62ba893542b7007f70eed4198fba8a74244ec93dca37905eab3d588636ca0e7
```

The source ZIP has 368 PCM WAV files over azimuths 0°–180°. The converter reconstructs the other hemisphere by mirroring azimuth and swapping left/right channels, producing the 710 directions described in the original documentation.

The data is Copyright 1994 MIT Media Laboratory. MIT provides it without usage restrictions and requests citation of Bill Gardner and Keith Martin when used in research or commercial applications. See <https://sound.media.mit.edu/resources/KEMAR.html> and `docs/mit-kemar.md` for provenance and technical details.

