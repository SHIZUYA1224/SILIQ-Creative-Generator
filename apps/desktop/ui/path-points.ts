/** Editable absolute geometry. Raw paths are never sanitized by removing commands. */
export interface PathPoint { x: number; y: number }
export type PathSegment =
  | { kind: 'move' | 'line'; to: PathPoint }
  | { kind: 'quad'; control: PathPoint; to: PathPoint }
  | { kind: 'cubic'; control1: PathPoint; control2: PathPoint; to: PathPoint }
  | { kind: 'close' };
export type PointField = 'to' | 'control' | 'control1' | 'control2';
export const MAX_PATH_COORDINATE = 1_000_000;
export const MAX_PATH_BYTES = 1024 * 1024;
export const MAX_PATH_SEGMENTS = 200_000;
export class PathSyntaxError extends Error {
  constructor(message: string, readonly offset: number) { super(message + '（位置 ' + offset + '）'); this.name = 'PathSyntaxError'; }
}
class Lexer {
  offset = 0;
  constructor(readonly text: string) {}
  skip() { while (this.offset < this.text.length && /[\s,]/.test(this.text[this.offset])) this.offset++; }
  done() { this.skip(); return this.offset >= this.text.length; }
  command() { this.skip(); const c = this.text[this.offset]; if (c && /[A-Za-z]/.test(c)) { this.offset++; return c; } return undefined; }
  number() {
    this.skip(); const match = /^[+-]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][+-]?\d+)?/.exec(this.text.slice(this.offset));
    if (!match) throw new PathSyntaxError('座標の数値が必要です', this.offset);
    const value = Number(match[0]); this.offset += match[0].length;
    if (!Number.isFinite(value) || Math.abs(value) > MAX_PATH_COORDINATE) throw new PathSyntaxError('座標は ±1,000,000 の有限値で指定してください', this.offset);
    return value;
  }
}
const point = (x = 0, y = 0): PathPoint => ({ x, y });
function finitePoint(p: PathPoint, offset: number) {
  if (!Number.isFinite(p.x) || !Number.isFinite(p.y) || Math.abs(p.x) > MAX_PATH_COORDINATE || Math.abs(p.y) > MAX_PATH_COORDINATE) throw new PathSyntaxError('変換後の座標が許可範囲を超えます', offset);
}
export function segmentPoints(segment: PathSegment): [PointField, PathPoint][] {
  switch (segment.kind) {
    case 'move': case 'line': return [['to', segment.to]];
    case 'quad': return [['control', segment.control], ['to', segment.to]];
    case 'cubic': return [['control1', segment.control1], ['control2', segment.control2], ['to', segment.to]];
    case 'close': return [];
  }
}
/** M/L/H/V/C/S/Q/T/Z, including relative forms, are expanded without rounding. */
export function parsePathPoints(data: string): PathSegment[] {
  if (data.length > MAX_PATH_BYTES || new TextEncoder().encode(data).length > MAX_PATH_BYTES) throw new PathSyntaxError('パスは 1 MiB 以内にしてください', 0);
  const lexer = new Lexer(data); const segments: PathSegment[] = [];
  let command: string | undefined; let current = point(); let start = point(); let cubic: PathPoint | undefined; let quad: PathPoint | undefined;
  while (!lexer.done()) {
    command = lexer.command() ?? command;
    if (!command) throw new PathSyntaxError('パス命令が必要です', lexer.offset);
    const relative = command === command.toLowerCase();
    const absolute = (p: PathPoint) => relative ? point(p.x + current.x, p.y + current.y) : p;
    const read = () => absolute(point(lexer.number(), lexer.number()));
    const reflect = (p?: PathPoint) => p ? point(2 * current.x - p.x, 2 * current.y - p.y) : { ...current };
    let segment: PathSegment; let nextCubic: PathPoint | undefined; let nextQuad: PathPoint | undefined;
    switch (command.toUpperCase()) {
      case 'M': current = read(); start = { ...current }; segment = { kind: 'move', to: current }; command = relative ? 'l' : 'L'; break;
      case 'L': current = read(); segment = { kind: 'line', to: current }; break;
      case 'H': { const x = lexer.number(); current = point(relative ? current.x + x : x, current.y); segment = { kind: 'line', to: current }; break; }
      case 'V': { const y = lexer.number(); current = point(current.x, relative ? current.y + y : y); segment = { kind: 'line', to: current }; break; }
      case 'C': { const control1 = read(); const control2 = read(); const to = read(); nextCubic = control2; current = to; segment = { kind: 'cubic', control1, control2, to }; break; }
      case 'S': { const control1 = reflect(cubic); const control2 = read(); const to = read(); nextCubic = control2; current = to; segment = { kind: 'cubic', control1, control2, to }; break; }
      case 'Q': { const control = read(); const to = read(); nextQuad = control; current = to; segment = { kind: 'quad', control, to }; break; }
      case 'T': { const control = reflect(quad); const to = read(); nextQuad = control; current = to; segment = { kind: 'quad', control, to }; break; }
      case 'Z': segment = { kind: 'close' }; current = { ...start }; command = undefined; break;
      case 'A': throw new PathSyntaxError('円弧 A/a は数値一覧の対象外です。ベジェ曲線に変換してください。元データは保持されます', lexer.offset);
      default: throw new PathSyntaxError('対応していないパス命令です。元データは保持されます', lexer.offset);
    }
    for (const [, p] of segmentPoints(segment)) finitePoint(p, lexer.offset);
    if (segments.length === 0 && segment.kind !== 'move') throw new PathSyntaxError('最初の点は M/m で指定してください', 0);
    segments.push(segment); cubic = nextCubic; quad = nextQuad;
    if (segments.length > MAX_PATH_SEGMENTS) throw new PathSyntaxError('パスの点数が上限を超えました', lexer.offset);
  }
  if (!segments.length) throw new PathSyntaxError('開始点 M が必要です', 0);
  return segments;
}
export function serializePathPoints(segments: readonly PathSegment[]): string {
  const pair = (p: PathPoint) => { finitePoint(p, 0); return String(p.x) + ' ' + String(p.y); };
  const text = segments.map(segment => {
    switch (segment.kind) {
      case 'move': return 'M' + pair(segment.to);
      case 'line': return 'L' + pair(segment.to);
      case 'quad': return 'Q' + pair(segment.control) + ' ' + pair(segment.to);
      case 'cubic': return 'C' + pair(segment.control1) + ' ' + pair(segment.control2) + ' ' + pair(segment.to);
      case 'close': return 'Z';
    }
  }).join(' ');
  parsePathPoints(text); return text;
}
export function updatePathPoint(segments: readonly PathSegment[], index: number, field: PointField, axis: 'x' | 'y', value: number): PathSegment[] {
  const next = structuredClone(segments) as PathSegment[];
  const segment = next[index];
  const entry = segment && segmentPoints(segment).find(([key]) => key === field);
  if (!entry) throw new Error('選択した制御点がありません');
  entry[1][axis] = value; finitePoint(entry[1], 0); return next;
}
export function appendPathSegment(segments: readonly PathSegment[], kind: 'move' | 'line' | 'quad' | 'cubic' | 'close'): PathSegment[] {
  const next = structuredClone(segments) as PathSegment[]; let current = point(); let start = point();
  for (const segment of next) {
    if (segment.kind === 'move') { current = segment.to; start = current; }
    else if (segment.kind === 'close') current = start;
    else current = segment.to;
  }
  const to = point(current.x + 40, current.y); let segment: PathSegment;
  switch (kind) {
    case 'move': segment = { kind: 'move', to: point(current.x + 20, current.y + 20) }; break;
    case 'line': segment = { kind: 'line', to }; break;
    case 'quad': segment = { kind: 'quad', control: point(current.x + 20, current.y - 20), to }; break;
    case 'cubic': segment = { kind: 'cubic', control1: point(current.x + 12, current.y - 20), control2: point(current.x + 28, current.y - 20), to }; break;
    case 'close': if (next.at(-1)?.kind === 'close') return next; segment = { kind: 'close' }; break;
  }
  for (const [, p] of segmentPoints(segment)) finitePoint(p, 0);
  next.push(segment); serializePathPoints(next); return next;
}
/** Removing a subpath start removes its complete subpath, never orphaned points. */
export function removePathSegment(segments: readonly PathSegment[], index: number): PathSegment[] {
  const next = structuredClone(segments) as PathSegment[];
  if (!next[index]) throw new Error('選択した点がありません');
  if (next[index].kind === 'move') {
    let end = index + 1; while (end < next.length && next[end].kind !== 'move') end++;
    next.splice(index, end - index);
  } else next.splice(index, 1);
  serializePathPoints(next); return next;
}
