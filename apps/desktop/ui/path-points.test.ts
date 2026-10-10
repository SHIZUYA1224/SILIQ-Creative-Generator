import { describe, expect, it } from 'vitest';
import { appendPathSegment, parsePathPoints, removePathSegment, serializePathPoints, updatePathPoint } from './path-points';

describe('bounded editable paths', () => {
  it('expands relative and smooth curves without losing compound contours', () => {
    const points = parsePathPoints('m10 10 c1 2 3 4 5 6 s3 4 5 6 z M30 30 q1 2 3 4 t5 6 Z');
    expect(serializePathPoints(points)).toBe('M10 10 C11 12 13 14 15 16 C17 18 18 20 20 22 Z M30 30 Q31 32 33 34 Q35 36 38 40 Z');
    expect(parsePathPoints(serializePathPoints(points))).toEqual(points);
  });
  it('keeps precise decimals, exponent notation and implicit line coordinates', () => {
    const points = parsePathPoints('M.1 -.2 1e2-3 h4 v5');
    expect(points).toEqual([{ kind: 'move', to: { x: .1, y: -.2 } }, { kind: 'line', to: { x: 100, y: -3 } }, { kind: 'line', to: { x: 104, y: -3 } }, { kind: 'line', to: { x: 104, y: 2 } }]);
  });
  it('rejects unsupported, incomplete, nonfinite and out-of-range paths', () => {
    for (const data of ['M0 0 A1 2 0 0 0 3 4', 'L0 0', 'M0', 'MNaN 0', 'M1e309 0', 'M0 0 l1000000 0 l1 0', 'M0 0 <script>', 'M0 0 Z 1 2']) expect(() => parsePathPoints(data)).toThrow();
    expect(() => parsePathPoints('M0 0 ' + ' '.repeat(1024 * 1024))).toThrow();
  });
  it('changes only the chosen handle and leaves caller state intact', () => {
    const points = parsePathPoints('M0 0 C10 20 30 40 50 60 Z');
    const changed = updatePathPoint(points, 1, 'control2', 'y', 42.125);
    expect(serializePathPoints(changed)).toBe('M0 0 C10 20 30 42.125 50 60 Z');
    expect(serializePathPoints(points)).toBe('M0 0 C10 20 30 40 50 60 Z');
  });
  it('adds and removes complete subpaths without orphaning points', () => {
    const points = parsePathPoints('M0 0 L10 10 Z M20 20 Q30 30 40 40 Z');
    expect(serializePathPoints(removePathSegment(points, 0))).toBe('M20 20 Q30 30 40 40 Z');
    const extended = appendPathSegment(points, 'move');
    expect(extended.at(-1)).toEqual({ kind: 'move', to: { x: 40, y: 40 } });
    expect(appendPathSegment(appendPathSegment(extended, 'line'), 'close').at(-1)).toEqual({ kind: 'close' });
    expect(() => removePathSegment(parsePathPoints('M0 0 L1 1 Z'), 0)).toThrow();
  });
});
