import { useEffect, useMemo, useRef, useState } from 'react';
import { LoaderCircle } from 'lucide-react';
import { call, issuesFrom, type Bounds, type DesignDocument, type Issue, type PreviewMetadata } from './api';
import { ancestorIds, drawNodes, frameDelta, isEffectivelyLocked, localDrag, nodeFrame, resizeBounds, resolvedNodes, resolvedStyles } from './geometry';

let nextPreviewRequest = 0;
export function Canvas({ sessionId, document, variantId, selected, zoom, grid, safe, snap, disabled, onSelect, onMove, onIssues }: {
  sessionId: string; document: DesignDocument; variantId: string; selected: string[]; zoom: number; grid: boolean; safe: boolean; snap: boolean; disabled: boolean;
  onSelect: (id: string | null, additive: boolean) => void;
  onMove: (changes: { id: string; bounds: Bounds }[]) => void;
  onIssues: (issues: Issue[]) => void;
}) {
  const [url, setUrl] = useState<string>();
  const [loading, setLoading] = useState(false);
  const [guides,setGuides]=useState(false);
  const [guideX,setGuideX]=useState(50),[guideY,setGuideY]=useState(50);
  const [drag, setDrag] = useState<{ ids: string[]; dx: number; dy: number }>();
  const [resizeDraft, setResizeDraft] = useState<{ id:string; bounds:Bounds }>();
  const frame = useRef<HTMLDivElement>(null);
  const issuesRef = useRef(onIssues); issuesRef.current = onIssues;
  const variant = document.variants.find(v => v.id === variantId) ?? document.variants[0];
  const nodes = useMemo(()=>resolvedNodes(document,variant.id),[document,variant.id]);
  const styles = useMemo(()=>resolvedStyles(document,variant.id),[document,variant.id]);
  const targets=useMemo(()=>[...drawNodes(nodes, document.baseArtboard.rootNodeId),...nodes.filter(n=>n.type==='group' && n.id!==document.baseArtboard.rootNodeId && selected.includes(n.id) && n.visible && !ancestorIds(nodes,n.id).some(id=>!nodes.find(parent=>parent.id===id)?.visible))],[nodes,document.baseArtboard.rootNodeId,selected]);
  const frames=useMemo(()=>new Map(targets.map(node=>[node.id,nodeFrame(nodes,node,styles)])),[targets,nodes,styles]);
  const locks=useMemo(()=>new Map(nodes.map(node=>[node.id,isEffectivelyLocked(nodes,node)])),[nodes]);
  const scale = Math.min(680 / variant.width, 660 / variant.height) * zoom;
  const activeUrl = useRef<string | undefined>(undefined);
  useEffect(() => {
    const requestId = ++nextPreviewRequest;
    let cancelled = false;
    setLoading(true);
    setUrl(undefined);
    void (async () => {
      try {
        const bytes = await call<ArrayBuffer>('request_preview', { sessionId, revision: document.revision, variantId: variant.id, requestId });
        const meta = await call<PreviewMetadata>('preview_metadata', { sessionId, requestId });
        if (cancelled || meta.documentRevision !== document.revision || meta.requestId !== requestId) return;
        if (meta.colorSpace !== 'sRGB' || meta.quality !== 'exact') throw new Error('正確プレビューの色空間または品質が一致しません。');
        if (bytes.byteLength !== meta.payloadLength || meta.width !== variant.width || meta.height !== variant.height) throw new Error('プレビューの寸法またはデータ長が一致しません。');
        let blob: Blob;
        if (meta.pixelFormat === 'RGBA8') {
          if (meta.stride !== meta.width * 4 || bytes.byteLength !== meta.height * meta.stride || meta.alphaType !== 'straight') throw new Error('プレビュー形式が不正です。');
          const canvas = window.document.createElement('canvas'); canvas.width = meta.width; canvas.height = meta.height;
          canvas.getContext('2d')!.putImageData(new ImageData(new Uint8ClampedArray(bytes), meta.width, meta.height), 0, 0);
          blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob(b => b ? resolve(b) : reject(new Error('プレビューを表示できません。'))));
        } else blob = new Blob([bytes], { type: 'image/png' });
        if (cancelled) return;
        const newUrl = URL.createObjectURL(blob);
        if (activeUrl.current) URL.revokeObjectURL(activeUrl.current);
        activeUrl.current = newUrl; setUrl(newUrl); issuesRef.current(meta.issues ?? []);
      } catch (error) { if (!cancelled) issuesRef.current(issuesFrom(error)); }
      finally { if (!cancelled) setLoading(false); }
    })();
    return () => { cancelled = true; };
  }, [sessionId, document.revision, variant.id, variant.width, variant.height]);
  useEffect(() => () => { if (activeUrl.current) URL.revokeObjectURL(activeUrl.current); }, []);

  return <div className="canvas-stage" onPointerDown={e => { if (e.target === e.currentTarget) onSelect(null, false); }}>
    <div className="artboard-caption"><span>{variant.label}</span><button aria-pressed={guides} onClick={()=>setGuides(!guides)}>ガイド</button><span>{variant.width} × {variant.height} px</span></div>{guides && <div className="guide-controls"><label>縦ガイド %<input type="number" aria-label="縦ガイドの位置" min={0} max={100} value={guideX} onChange={e=>setGuideX(Math.max(0,Math.min(100,Number(e.target.value))))} /></label><label>横ガイド %<input type="number" aria-label="横ガイドの位置" min={0} max={100} value={guideY} onChange={e=>setGuideY(Math.max(0,Math.min(100,Number(e.target.value))))} /></label></div>}
    <div className="artboard-surround"><div className="ruler-horizontal">{Array.from({length:6},(_,i)=><span key={i}>{Math.round(variant.width*i/5)}</span>)}</div><div className="ruler-vertical">{Array.from({length:6},(_,i)=><span key={i}>{Math.round(variant.height*i/5)}</span>)}</div><div ref={frame} className="artboard" style={{ width: variant.width * scale, height: variant.height * scale }}>
      {url ? <img className="native-preview" src={url} alt={`${document.title} の正確プレビュー`} draggable={false} /> : <div className="preview-placeholder">{loading ? <><LoaderCircle className="spin" size={24} />描画しています</> : '検査項目を修正するとプレビューが表示されます'}</div>}
      {guides && <><div className="guide-line vertical" style={{left:`${guideX}%`}} /><div className="guide-line horizontal" style={{top:`${guideY}%`}} /></>}{grid && <div className="grid-overlay" style={{ backgroundSize: `${24 * scale}px ${24 * scale}px` }} />}
      {safe && <div className="safe-overlay" style={{ inset: '5%' }}><span>SAFE AREA 5%</span></div>}
      {targets.map(node => {
        const moved = drag?.ids.includes(node.id);
        const visualNode = resizeDraft?.id===node.id ? {...node,bounds:resizeDraft.bounds} : node;
        const position = resizeDraft?.id===node.id ? nodeFrame(nodes,visualNode,styles) : frames.get(node.id)!;
        return <div key={node.id} role="button" tabIndex={0} aria-label={`レイヤー ${node.name}`} aria-pressed={selected.includes(node.id)}
          className={`node-target ${selected.includes(node.id) ? 'selected' : ''} ${locks.get(node.id) ? 'locked' : ''}`}
          style={{ left: (position.x + (moved ? drag?.dx ?? 0 : 0)) * scale, top: (position.y + (moved ? drag?.dy ?? 0 : 0)) * scale, width: visualNode.bounds.width * scale, height: visualNode.bounds.height * scale, transformOrigin: '0 0', transform: `matrix(${position.a}, ${position.b}, ${position.c}, ${position.d}, 0, 0)` }}
          onKeyDown={e => { if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); onSelect(node.id, e.metaKey || e.shiftKey); } }}
          onPointerDown={e => {
            e.stopPropagation(); onSelect(node.id, e.metaKey || e.shiftKey);
            if (disabled || locks.get(node.id) || e.metaKey || e.shiftKey) return;
            const ids = (selected.includes(node.id) ? selected : [node.id]).filter(id=>!locks.get(id)&&!ancestorIds(nodes,id).some(parent=>selected.includes(parent)));
            const start = { x: e.clientX, y: e.clientY };
            const target = e.currentTarget; target.setPointerCapture(e.pointerId);
            let delta = { dx: 0, dy: 0 };
            const move = (event: PointerEvent) => {
              const rawX = (event.clientX - start.x) / scale, rawY = (event.clientY - start.y) / scale;
              const gridStep = snap ? 8 : 1;
              delta = { dx: Math.round(rawX / gridStep) * gridStep, dy: Math.round(rawY / gridStep) * gridStep };
              setDrag({ ids, ...delta });
            };
            const finish = (event: PointerEvent) => {
              target.removeEventListener('pointermove', move); target.removeEventListener('pointerup', finish); target.removeEventListener('pointercancel', cancel);
              if (target.hasPointerCapture(event.pointerId)) target.releasePointerCapture(event.pointerId);
              setDrag(undefined);
              if (delta.dx || delta.dy) onMove(nodes.filter(n => ids.includes(n.id)).map(n => { const local = localDrag(nodes,n,delta.dx,delta.dy,styles); return { id: n.id, bounds: { ...n.bounds, x: n.bounds.x + local.dx, y: n.bounds.y + local.dy } }; }));
            };
            const cancel = () => { target.removeEventListener('pointermove', move); target.removeEventListener('pointerup', finish); target.removeEventListener('pointercancel', cancel); setDrag(undefined); };
            target.addEventListener('pointermove', move); target.addEventListener('pointerup', finish); target.addEventListener('pointercancel', cancel);
          }}>
          {selected.includes(node.id) && <><i className="handle tl" /><i className="handle tr" /><i className="handle bl" /><span className="selection-label">{node.name}</span>{!disabled && !locks.get(node.id) && <button className="node-resize-handle" aria-label={`${node.name}の拡大縮小`} onKeyDown={e=>{if(['ArrowRight','ArrowDown','ArrowLeft','ArrowUp'].includes(e.key)){e.preventDefault();e.stopPropagation();const step=e.shiftKey?10:1;onMove([{id:node.id,bounds:resizeBounds(node,e.key==='ArrowRight'?step:e.key==='ArrowLeft'?-step:0,e.key==='ArrowDown'?step:e.key==='ArrowUp'?-step:0)}]);}}} onPointerDown={e=>{
            e.stopPropagation(); const target=e.currentTarget;target.setPointerCapture(e.pointerId);
            const start={x:e.clientX,y:e.clientY};let bounds=node.bounds;
            const move=(event:PointerEvent)=>{const worldX=(event.clientX-start.x)/scale,worldY=(event.clientY-start.y)/scale;let {dx:dw,dy:dh}=frameDelta(position,worldX,worldY);if(event.shiftKey)dh=dw*node.bounds.height/node.bounds.width;const step=snap?8:1;bounds=resizeBounds(node,Math.round(dw/step)*step,Math.round(dh/step)*step);setResizeDraft({id:node.id,bounds});};
            const finish=(event:PointerEvent)=>{target.removeEventListener('pointermove',move);target.removeEventListener('pointerup',finish);target.removeEventListener('pointercancel',cancel);if(target.hasPointerCapture(event.pointerId))target.releasePointerCapture(event.pointerId);setResizeDraft(undefined);if(bounds.width!==node.bounds.width||bounds.height!==node.bounds.height)onMove([{id:node.id,bounds}]);};
            const cancel=()=>{target.removeEventListener('pointermove',move);target.removeEventListener('pointerup',finish);target.removeEventListener('pointercancel',cancel);setResizeDraft(undefined);};target.addEventListener('pointermove',move);target.addEventListener('pointerup',finish);target.addEventListener('pointercancel',cancel);
          }} />}</>}
        </div>;
      })}
      {loading && url && <div className="preview-updating"><LoaderCircle size={13} className="spin" />更新中</div>}
    </div>
    </div><p className="canvas-note">Skia の共通エンジンによる正確プレビュー · sRGB</p>
  </div>;
}
