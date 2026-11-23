import { describe, expect, it } from 'vitest';
import { MAX_CURVE_POINTS, canAddCurvePoint } from './curveLimit';

describe('curve point limit', () => {
  it('allows a point up to the limit and refuses the next one', () => {
    expect(canAddCurvePoint(MAX_CURVE_POINTS - 1)).toBe(true);
    expect(canAddCurvePoint(MAX_CURVE_POINTS)).toBe(false);
  });
});
