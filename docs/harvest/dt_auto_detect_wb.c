/* ============================================================================
   HARVEST REFERENCE - not compiled, kept for traceability.

   Source : darktable  src/iop/channelmixerrgb.c
   Commit : 98a9ade9095a449cecd0423509952ecc4a00d890
   Fetched: 2026-09-08
   Function: _auto_detect_WB()

   Automatic illuminant detection from the image itself - darktable's
   "detect from image surfaces" and "detect from image edges". This is the
   feature AK wants; Argentum already has spot/click white balance.

   Two modes:
     SURFACES - weighted average of chromaticities over flat areas
     EDGES    - laplacian-weighted grey-edge hypothesis

   Based on:
     A Fast White Balance Algorithm Based on Pixel Greyness
       Ba Thai, Guang Deng, Robert Ross
     Edge-Based Color Constancy
       Joost van de Weijer, Theo Gevers, Arjan Gijsenij

   No darktable pipeline dependencies: pixels -> XYZ -> xy chromaticity ->
   weighted average -> illuminant. Ports to Rust as-is.

   Licence: darktable is GPL-3.0. See ../ARCHITECTURE.md.
   When ported, credit darktable in Special Thanks (already listed) and add
   the CHANGELOG entry naming this commit.
   ============================================================================ */
static inline void _auto_detect_WB(const float *const restrict in,
                                   float *const restrict temp,
                                   dt_illuminant_t illuminant,
                                   const size_t width,
                                   const size_t height,
                                   const size_t ch,
                                   const dt_colormatrix_t RGB_to_XYZ,
                                   dt_aligned_pixel_t xyz)
{
   /* Detect the chromaticity of the illuminant based on the grey edges hypothesis.
      So we compute a laplacian filter and get the weighted average of its chromaticities

      Inspired by :
      A Fast White Balance Algorithm Based on Pixel Greyness, Ba Thai·Guang Deng·Robert Ross
      https://www.researchgate.net/profile/Ba_Son_Thai/publication/308692177_A_Fast_White_Balance_Algorithm_Based_on_Pixel_Greyness/

      Edge-Based Color Constancy, Joost van de Weijer, Theo Gevers, Arjan Gijsenij
      https://hal.inria.fr/inria-00548686/document
    */
    const float D50[2] = { D50xyY.x, D50xyY.y };
// Convert RGB to xy
  DT_OMP_FOR(collapse(2))
  for(size_t i = 0; i < height; i++)
    for(size_t j = 0; j < width; j++)
    {
      const size_t index = (i * width + j) * ch;
      dt_aligned_pixel_t RGB;
      dt_aligned_pixel_t XYZ;

      // Clip negatives
      for_each_channel(c,aligned(in))
        RGB[c] = fmaxf(in[index + c], 0.0f);

      // Convert to XYZ
      dot_product(RGB, RGB_to_XYZ, XYZ);

      // Convert to xyY
      const float sum = fmaxf(XYZ[0] + XYZ[1] + XYZ[2], NORM_MIN);
      XYZ[0] /= sum;   // x
      XYZ[2] = XYZ[1]; // Y
      XYZ[1] /= sum;   // y

      // Shift the chromaticity plane so the D50 point (target) becomes the origin
      const float norm = dt_fast_hypotf(D50[0], D50[1]);

      temp[index    ] = (XYZ[0] - D50[0]) / norm;
      temp[index + 1] = (XYZ[1] - D50[1]) / norm;
      temp[index + 2] =  XYZ[2];
    }

  float elements = 0.f;
  dt_aligned_pixel_t xyY = { 0.f };

  if(illuminant == DT_ILLUMINANT_DETECT_SURFACES)
  {
    DT_OMP_FOR(reduction(+:xyY, elements))
    for(size_t i = 2 * OFF; i < height - 4 * OFF; i += OFF)
      for(size_t j = 2 * OFF; j < width - 4 * OFF; j += OFF)
      {
        float DT_ALIGNED_PIXEL central_average[2];

        #pragma unroll
        for(size_t c = 0; c < 2; c++)
        {
          // B-spline local average / blur
          central_average[c] = (temp[SHF(-OFF, -OFF, c)]
                                + 2.f * temp[SHF(-OFF, 0, c)]
                                + temp[SHF(-OFF, +OFF, c)]
                                + 2.f * temp[SHF(   0, -OFF, c)]
                                + 4.f * temp[SHF(   0, 0, c)]
                                + 2.f * temp[SHF(   0, +OFF, c)]
                                + temp[SHF(+OFF, -OFF, c)]
                                + 2.f * temp[SHF(+OFF, 0, c)]
                                + temp[SHF(+OFF, +OFF, c)]) / 16.0f;
          central_average[c] = fmaxf(central_average[c], 0.0f);
        }

        dt_aligned_pixel_t var = { 0.f };

        // compute patch-wise variance
        // If variance = 0, we are on a flat surface and want to discard that patch.
        #pragma unroll
        for(size_t c = 0; c < 2; c++)
        {
          var[c] = (  sqf(temp[SHF(-OFF, -OFF, c)] - central_average[c])
                    + sqf(temp[SHF(-OFF,    0, c)] - central_average[c])
                    + sqf(temp[SHF(-OFF, +OFF, c)] - central_average[c])
                    + sqf(temp[SHF(0,    -OFF, c)] - central_average[c])
                    + sqf(temp[SHF(0,       0, c)] - central_average[c])
                    + sqf(temp[SHF(0,    +OFF, c)] - central_average[c])
                    + sqf(temp[SHF(+OFF, -OFF, c)] - central_average[c])
                    + sqf(temp[SHF(+OFF,    0, c)] - central_average[c])
                    + sqf(temp[SHF(+OFF, +OFF, c)] - central_average[c])
                    ) / 9.0f;
        }

        // Compute the patch-wise chroma covariance.
        // If covariance = 0, chroma channels are not correlated and we either have noise or chromatic aberrations.
        // Both ways, we want to discard that patch from the chroma average.
        var[2] = ((temp[SHF(-OFF, -OFF, 0)] - central_average[0]) * (temp[SHF(-OFF, -OFF, 1)] - central_average[1]) +
                  (temp[SHF(-OFF,    0, 0)] - central_average[0]) * (temp[SHF(-OFF,    0, 1)] - central_average[1]) +
                  (temp[SHF(-OFF, +OFF, 0)] - central_average[0]) * (temp[SHF(-OFF, +OFF, 1)] - central_average[1]) +
                  (temp[SHF(   0, -OFF, 0)] - central_average[0]) * (temp[SHF(   0, -OFF, 1)] - central_average[1]) +
                  (temp[SHF(   0,    0, 0)] - central_average[0]) * (temp[SHF(   0,    0, 1)] - central_average[1]) +
                  (temp[SHF(   0, +OFF, 0)] - central_average[0]) * (temp[SHF(   0, +OFF, 1)] - central_average[1]) +
                  (temp[SHF(+OFF, -OFF, 0)] - central_average[0]) * (temp[SHF(+OFF, -OFF, 1)] - central_average[1]) +
                  (temp[SHF(+OFF,    0, 0)] - central_average[0]) * (temp[SHF(+OFF,    0, 1)] - central_average[1]) +
                  (temp[SHF(+OFF, +OFF, 0)] - central_average[0]) * (temp[SHF(+OFF, +OFF, 1)] - central_average[1])
          ) / 9.0f;

        // Compute the Minkowski p-norm for regularization
        const float p = 8.f;
        const float p_norm
            = powf(powf(fabsf(central_average[0]), p)
                   + powf(fabsf(central_average[1]), p), 1.f / p) + NORM_MIN;
        const float weight = var[0] * var[1] * var[2];

        #pragma unroll
        for(size_t c = 0; c < 2; c++) xyY[c] += central_average[c] * weight / p_norm;
        elements += weight / p_norm;
      }
  }
  else if(illuminant == DT_ILLUMINANT_DETECT_EDGES)
  {
    DT_OMP_FOR(reduction(+:xyY, elements))
    for(size_t i = 2 * OFF; i < height - 4 * OFF; i += OFF)
      for(size_t j = 2 * OFF; j < width - 4 * OFF; j += OFF)
      {
        float DT_ALIGNED_PIXEL dd[2];
        float DT_ALIGNED_PIXEL central_average[2];

        #pragma unroll
        for(size_t c = 0; c < 2; c++)
        {
          // B-spline local average / blur
          central_average[c] = (temp[SHF(-OFF, -OFF, c)]
                                + 2.f * temp[SHF(-OFF, 0, c)]
                                + temp[SHF(-OFF, +OFF, c)]
                                + 2.f * temp[SHF(   0, -OFF, c)]
                                + 4.f * temp[SHF(   0, 0, c)]
                                + 2.f * temp[SHF(   0, +OFF, c)]
                                + temp[SHF(+OFF, -OFF, c)]
                                + 2.f * temp[SHF(+OFF, 0, c)]
                                + temp[SHF(+OFF, +OFF, c)]) / 16.0f;

          // image - blur = laplacian = edges
          dd[c] = temp[SHF(0, 0, c)] - central_average[c];
        }

        // Compute the Minkowski p-norm for regularization
        const float p = 8.f;
        const float p_norm = powf(powf(fabsf(dd[0]), p)
                                  + powf(fabsf(dd[1]), p), 1.f / p) + NORM_MIN;

#pragma unroll
        for(size_t c = 0; c < 2; c++) xyY[c] -= dd[c] / p_norm;
        elements += 1.f;
      }
  }

  const float norm_D50 = dt_fast_hypotf(D50[0], D50[1]);

  for(size_t c = 0; c < 2; c++)
    xyz[c] = norm_D50 * (xyY[c] / elements) + D50[c];
}

#if defined(__GNUC__) && defined(_WIN32)
  #pragma GCC pop_options
#endif

#endif // AI_ACTIVATED

static void _declare_cat_on_pipe(dt_iop_module_t *self, const gboolean preset)
{
  // Avertise in dev->chroma that we are doing chromatic adaptation here
  // preset = TRUE allows to capture the CAT a priori at init time
  const dt_iop_channelmixer_rgb_params_t *p = self->params;
  const dt_iop_channelmixer_rgb_gui_data_t *g = self->gui_data;
  if(!g) return;

  dt_dev_chroma_t *chr = &self->dev->chroma;
  const dt_iop_module_t *origcat = chr->adaptation;

  if(preset
    || (self->enabled
        && !g->is_blending
        && !(p->adaptation == DT_ADAPTATION_RGB || p->illuminant == DT_ILLUMINANT_PIPE)))
  {
    // We do CAT here so we need to register this instance as CAT-handler.
