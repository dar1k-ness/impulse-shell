// C ABI over QuantizeCelebi from materialyoucolor 2.0.10 celebi.cc (pybind11 removed).
#include <cstddef>
#include <cstdint>
#include <vector>
#include "utils.h"
#include "wsmeans.h"
#include "wu.h"

extern "C" size_t colorgen_quantize_celebi(const uint32_t* rgb, size_t n, int max_colors,
                                           uint32_t* out_argb, uint32_t* out_count,
                                           size_t cap) {
  if (max_colors > 256) max_colors = 256;
  std::vector<material_color_utilities::Argb> opaque_pixels(rgb, rgb + n);
  std::vector<material_color_utilities::Argb> wu_result =
      material_color_utilities::QuantizeWu(opaque_pixels, max_colors);
  material_color_utilities::QuantizerResult result =
      material_color_utilities::QuantizeWsmeans(opaque_pixels, wu_result, max_colors);
  size_t i = 0;
  for (const auto& [argb, count] : result.color_to_count) {
    if (i < cap) {
      out_argb[i] = argb;
      out_count[i] = count;
    }
    i++;
  }
  return i;
}
