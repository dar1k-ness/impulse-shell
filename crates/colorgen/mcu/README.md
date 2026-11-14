Google material-color-utilities C++ quantizer (Wu + WSMeans), Apache-2.0, copied verbatim
from materialyoucolor 2.0.10 (`materialyoucolor/quantize/`), the build the original
`generate_colors_material.py` runs. Kept as C++ so the palette seed matches it bit for bit
(including glibc `rand()` seeding in WSMeans). `celebi_ffi.cc` mirrors its `QuantizeCelebi`.
