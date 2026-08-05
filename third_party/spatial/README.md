# Sonux spatial audio data

Sonux embeds one audited Aalto University SOFA dataset for its production
7.1-to-binaural renderer. No optional comparison engines or development
binaries are included in this repository.

## Aalto near-field HRTF

- Publisher: Aalto University
- Record: <https://zenodo.org/records/7316545>
- DOI: <https://doi.org/10.5281/zenodo.7316545>
- File: `runtime/NF_LIB_HRTF_LFE.sofa`
- Publisher MD5: `3e5b96cbbe2e7e6b39b9d1198cbabfba`
- SHA-256: `07e5bb16509b69043804e700461ced1b43b326b1c3a0c2e782d5d38344db5c7f`
- License: CC BY 4.0
- Related paper: <https://doi.org/10.1016/j.apacoust.2022.109173>

The repository contains only the required 1.4 MB low-frequency-extended SOFA
file and its attribution. Its metadata reports 48 kHz FIR data, 196 source
positions, two receivers, 512 samples per impulse response, and four measured
radii: 0.2, 0.3, 0.4, and 0.5 metres. The low-frequency extension crosses to a
rigid-sphere model at 500 Hz.

The libmysofa 1.3.4 validator returns error 10012 when opening this 2023
dataset directly because it expresses `ReceiverPosition` in spherical rather
than Cartesian coordinates. Sonux converts the loaded in-memory structure to
Cartesian coordinates and validates it before constructing the production
lookup and interpolation tables. The tracked SOFA file itself is not changed.

The production build embeds `runtime/NF_LIB_HRTF_LFE.sofa`; the tracked copy
must retain the SHA-256 listed above.

## Verify the dataset

```sh
sha256sum third_party/spatial/runtime/NF_LIB_HRTF_LFE.sofa
```

Any mismatch means the file should not be used until its origin and contents
have been checked again.
