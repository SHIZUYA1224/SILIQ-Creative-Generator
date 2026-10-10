import { useEffect, useMemo, useState } from 'react';
import type { EditCommand, Node } from './api';
import { Field, NumberField, Section } from './fields';
import { appendPathSegment, parsePathPoints, removePathSegment, segmentPoints, serializePathPoints, updatePathPoint, type PathSegment, type PointField } from './path-points';

type PathNode = Extract<Node, { type: 'path' }>;
const kindLabel = { move: '開始点', line: '直線', quad: '二次ベジェ', cubic: '三次ベジェ', close: '閉じる' };
const pointLabel: Record<PointField, string> = { to: 'アンカー', control: 'ハンドル', control1: 'ハンドル 1', control2: 'ハンドル 2' };
const visibleLimit = 250;
/** Numeric anchor/handle editing is one typed transaction per confirmed change. */
export function PathEditor({ node, disabled, edit }: { node: PathNode; disabled: boolean; edit: (command: EditCommand) => void }) {
  const [draft, setDraft] = useState(node.data);
  const [operationError, setOperationError] = useState('');
  useEffect(() => { setDraft(node.data); setOperationError(''); }, [node.id, node.data]);
  const parsed = useMemo(() => {
    try { return { segments: parsePathPoints(draft), error: '' }; }
    catch (error) { return { segments: undefined, error: error instanceof Error ? error.message : String(error) }; }
  }, [draft]);
  const emit = (data: string) => { setDraft(data); setOperationError(''); edit({ op: 'setPathData', nodeId: node.id, data }); };
  const mutate = (action: (segments: PathSegment[]) => PathSegment[]) => {
    if (!parsed.segments || disabled) return;
    try { emit(serializePathPoints(action(parsed.segments))); }
    catch (error) { setOperationError(error instanceof Error ? error.message : String(error)); }
  };
  const error = operationError || parsed.error;
  return <>
    <Section title="パスのアンカーとハンドル">
      <p className="helper">座標を数値で指定します。ベジェ曲線はアンカーとハンドルを別々に編集できます。</p>
      <Field label="塗り規則"><select value={node.fillRule} disabled={disabled} onChange={event => edit({ op: 'setPathFillRule', nodeId: node.id, fillRule: event.target.value })}><option value="non-zero">非ゼロ（輪郭の方向）</option><option value="even-odd">偶奇（重なりを中抜き）</option></select></Field>
      {error && <p className="helper" role="alert">{error}</p>}
      {parsed.segments && <>
        <div className="path-point-list" style={{ maxHeight: 420, overflowY: 'auto' }}>
          {parsed.segments.slice(0, visibleLimit).map((segment, index) => <div className="property-section path-point" key={index}>
            <div className="field-grid"><strong>{index + 1}. {kindLabel[segment.kind]}</strong><button type="button" className="button secondary" disabled={disabled || (segment.kind === 'move' && parsed.segments!.filter(s => s.kind === 'move').length === 1)} onClick={() => mutate(segments => removePathSegment(segments, index))}>{segment.kind === 'move' ? 'サブパスを削除' : '点を削除'}</button></div>
            {segmentPoints(segment).map(([field, point]) => <div className="field-grid" key={field}>
              <NumberField key={point.x + '-x'} label={'点 ' + (index + 1) + ' ' + pointLabel[field] + ' X'} value={point.x} min={-1_000_000} max={1_000_000} step={.1} disabled={disabled} onCommit={value => mutate(segments => updatePathPoint(segments, index, field, 'x', value))} />
              <NumberField key={point.y + '-y'} label={'点 ' + (index + 1) + ' ' + pointLabel[field] + ' Y'} value={point.y} min={-1_000_000} max={1_000_000} step={.1} disabled={disabled} onCommit={value => mutate(segments => updatePathPoint(segments, index, field, 'y', value))} />
            </div>)}
          </div>)}
        </div>
        {parsed.segments.length > visibleLimit && <p className="helper">全 {parsed.segments.length} 命令のうち先頭 {visibleLimit} 件を表示しています。後続の点は下のパスデータで編集できます。編集していない点も保持します。</p>}
        <div className="field-grid">
          <button type="button" className="button secondary" disabled={disabled} onClick={() => mutate(segments => appendPathSegment(segments, 'line'))}>直線を追加</button>
          <button type="button" className="button secondary" disabled={disabled} onClick={() => mutate(segments => appendPathSegment(segments, 'quad'))}>二次ベジェを追加</button>
          <button type="button" className="button secondary" disabled={disabled} onClick={() => mutate(segments => appendPathSegment(segments, 'cubic'))}>三次ベジェを追加</button>
          <button type="button" className="button secondary" disabled={disabled} onClick={() => mutate(segments => appendPathSegment(segments, 'move'))}>サブパスを追加</button>
          <button type="button" className="button secondary" disabled={disabled || parsed.segments.at(-1)?.kind === 'close'} onClick={() => mutate(segments => appendPathSegment(segments, 'close'))}>最後のサブパスを閉じる</button>
        </div>
      </>}
    </Section>
    <Section title="パスデータ">
      <Field label="輪郭データ"><textarea value={draft} rows={5} disabled={disabled} spellCheck={false} onChange={event => { setDraft(event.target.value); setOperationError(''); }} /></Field>
      <button type="button" className="button secondary full-width" disabled={disabled || !!parsed.error || draft === node.data} onClick={() => emit(draft)}>パスデータを反映</button>
      <p className="helper">M/L/C/Q/Z とその相対形式、H/V/S/T に対応します。未対応の命令を削って取り込むことはありません。</p>
    </Section>
    <Section title="パスの元座標と拡大縮小">
      <p className="helper">元座標の範囲を配置ボックスへ変換します。輪郭データは保持します。</p>
      <div className="field-grid">{(['x', 'y', 'width', 'height'] as const).map(field => <NumberField key={field + '-' + node.viewBox[field]} label={'元範囲 ' + ({ x: 'X', y: 'Y', width: '幅', height: '高さ' })[field]} value={node.viewBox[field]} min={field === 'width' || field === 'height' ? .01 : -1_000_000} max={field === 'width' || field === 'height' ? 16384 : 1_000_000} step={.1} disabled={disabled} onCommit={value => edit({ op: 'setPathViewBox', nodeId: node.id, viewBox: { ...node.viewBox, [field]: value } })} />)}</div>
    </Section>
  </>;
}
