/* ============================================================================
   HARVEST REFERENCE - not compiled, kept for traceability.

   Source : darktable  src/develop/masks/gradient.c
   Commit : cb30520d0a5e23f094ef98d0578d903ddaa64804
   Fetched: 2026-10-09
   Function: _gradient_get_mask() - the fall-off LUT and the loop that reads it
             _gradient_get_points_border() - where the border lines are drawn

   darktable's gradient mask fades as 0.5 + 0.5 * erf(distance / compression)
   (its "sigmoidal" state, the default; "linear" is the straight ramp RapidRAW
   uses). The border lines are drawn at +/-compression, so the curve is 92%
   and 8% at the lines and continues past them; it is cut at 4 * compression.

   Ported to src-tauri/src/mods/mask_falloff.rs as mask_falloff::linear.
   RapidRAW's `range` (centre line to outer handle) takes the role of
   `compression`. Curvature, which bends the line, was not taken.

   Not taken: ellipse.c and circle.c fall off as f * f, which keeps a corner
   at the inner edge. The radial mask uses smoothstep instead.

   Licence: darktable is GPL-3.0. See ../ARCHITECTURE.md.
   Credited in src/argentum/credits.ts; CHANGELOG 26.41.9 names this commit.
   ============================================================================ */

/* --- _gradient_get_mask(), lines 1137-1196 --- */
  // we calculate the mask at grid points and recycle point buffer to store results
  const float wd = piece->pipe->iwidth;
  const float ht = piece->pipe->iheight;
  const float hwscale = 1.0f / dt_fast_hypotf(wd, ht);
  const float ihwscale = 1.0f / hwscale;
  const float v = deg2radf(-gradient->rotation);
  const float sinv = sinf(v);
  const float cosv = cosf(v);
  const float xoffset = cosv * gradient->anchor[0] * wd + sinv * gradient->anchor[1] * ht;
  const float yoffset = sinv * gradient->anchor[0] * wd - cosv * gradient->anchor[1] * ht;
  const float compression = fmaxf(gradient->compression, 0.001f);
  const float normf = 1.0f / compression;
  const float curvature = gradient->curvature;
  const dt_masks_gradient_states_t state = gradient->state;

  const int lutmax = ceilf(4 * compression * ihwscale);
  const int lutsize = 2 * lutmax + 2;
  float *lut = dt_alloc_align_float((size_t)lutsize);
  if(lut == NULL)
  {
    dt_free_align(points);
    return 0;
  }

  DT_OMP_FOR()
  for(int n = 0; n < lutsize; n++)
  {
    const float distance = (n - lutmax) * hwscale;
    const float value = 0.5f
      + 0.5f * ((state == DT_MASKS_GRADIENT_STATE_LINEAR)
                ? normf * distance
                : erff(distance / compression));
    lut[n] = (value < 0.0f) ? 0.0f : ((value > 1.0f) ? 1.0f : value);
  }

  // center lut around zero
  const float *clut = lut + lutmax;


  DT_OMP_FOR(collapse(2))
  for(int j = 0; j < gh; j++)
  {
    for(int i = 0; i < gw; i++)
    {
      const float x = points[(j * gw + i) * 2];
      const float y = points[(j * gw + i) * 2 + 1];

      const float x0 = (cosv * x + sinv * y - xoffset) * hwscale;
      const float y0 = (sinv * x - cosv * y - yoffset) * hwscale;

      const float distance = y0 - curvature * x0 * x0;

      points[(j * gw + i) * 2] = (distance <= -4.0f * compression) ? 0.0f :
                                    ((distance >= 4.0f * compression)
                                     ? 1.0f
                                     : dt_gradient_lookup(clut, distance * ihwscale));
    }
  }

  dt_free_align(lut);

/* --- _gradient_get_points_border(), lines 1004-1027 --- */
static int _gradient_get_points_border(dt_develop_t *dev,
                                       dt_masks_form_t *form,
                                       float **points,
                                       int *points_count,
                                       float **border,
                                       int *border_count,
                                       const int source,
                                       const dt_iop_module_t *module)
{
  (void)source;  // unused arg, keep compiler from complaining
  const dt_masks_point_gradient_t *gradient = form->points->data;
  if(_gradient_get_points(dev, gradient->anchor[0], gradient->anchor[1],
                          gradient->rotation, gradient->curvature,
                          points, points_count))
  {
    if(border)
      return _gradient_get_pts_border(dev, gradient->anchor[0], gradient->anchor[1],
                                      gradient->rotation,
                                      gradient->compression, gradient->curvature,
                                      border, border_count);
    else
      return 1;
  }
  return 0;
}
