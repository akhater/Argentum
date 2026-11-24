/**
 * The most points one tone curve may have: the size of the shader's arrays. Defined in Rust as
 * `MAX_CURVE_POINTS` (mods/curve_limit.rs); a test there keeps this equal.
 */
export const MAX_CURVE_POINTS = 16;

/** Whether a curve with `count` points may take one more. */
export const canAddCurvePoint = (count: number): boolean => count < MAX_CURVE_POINTS;
