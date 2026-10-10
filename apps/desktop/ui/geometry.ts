import type { Bounds, DesignDocument, Node } from './api';

export function resolvedNodes(doc: DesignDocument, variantId: string): Node[] {
  const patches = new Map(doc.variants.find(v => v.id === variantId)?.nodePatches.map(p => [p.nodeId, p]));
  return doc.nodes.map(node => {
    const p = patches.get(node.id);
    if (!p) return node;
    return { ...node, bounds: p.bounds, ...(node.type === 'text' ? { text: { ...node.text, ...(p.fontSize === undefined ? {} : { fontSize: p.fontSize }), ...(p.lineHeight === undefined ? {} : { lineHeight: p.lineHeight }) } } : {}) } as Node;
  });
}
export function resolvedStyles(doc: DesignDocument, variantId: string): Styles {
  const result = (doc.nodeStyles ?? []).map(style => ({...style}));
  for (const patch of doc.variants.find(v => v.id === variantId)?.nodePatches ?? []) {
    if (!patch.affine) continue;
    const existing = result.find(style => style.nodeId === patch.nodeId);
    if (existing) existing.affine = patch.affine;
    else result.push({nodeId:patch.nodeId,blendMode:'normal',effects:[],affine:patch.affine});
  }
  return result;
}
export function ancestorIds(nodes: Node[], nodeId: string): string[] {
  const parents = new Map(nodes.flatMap(n => n.type === 'group' ? n.childIds.map(id => [id, n.id] as const) : []));
  const result: string[] = [];
  for (let current = parents.get(nodeId); current && result.length < 32; current = parents.get(current)) result.push(current);
  return result;
}
export function isEffectivelyLocked(nodes: Node[], node: Node): boolean {
  return node.locked || ancestorIds(nodes, node.id).some(id => nodes.find(n => n.id === id)?.locked);
}
export function drawNodes(nodes: Node[], rootId: string): Node[] {
  const byId = new Map(nodes.map(node => [node.id, node]));
  const result: Node[] = [];
  const visit = (id: string, seen: Set<string>) => {
    if (seen.has(id) || seen.size >= 2000) return;
    seen.add(id);
    const node = byId.get(id); if (!node || !node.visible) return;
    if (node.type === 'group') node.childIds.forEach(child => visit(child, seen));
    else result.push(node);
  };
  visit(rootId, new Set()); return result;
}
export function layerTree(nodes: Node[], rootId: string): {node:Node; depth:number}[] {
  const byId=new Map(nodes.map(node=>[node.id,node])),result:{node:Node;depth:number}[]=[];
  const seen=new Set<string>();
  const visit=(id:string,depth:number)=>{if(seen.has(id)||seen.size>=2000)return;seen.add(id);const node=byId.get(id);if(!node)return;result.push({node,depth});if(node.type==='group')[...node.childIds].reverse().forEach(child=>visit(child,depth+1));};
  const root=byId.get(rootId);if(root?.type==='group')[...root.childIds].reverse().forEach(id=>visit(id,0));return result;
}
type Matrix = [number, number, number, number, number, number];
const identity: Matrix = [1, 0, 0, 1, 0, 0];
function multiply(a: Matrix, b: Matrix): Matrix {
  return [a[0]*b[0]+a[2]*b[1],a[1]*b[0]+a[3]*b[1],a[0]*b[2]+a[2]*b[3],a[1]*b[2]+a[3]*b[3],a[0]*b[4]+a[2]*b[5]+a[4],a[1]*b[4]+a[3]*b[5]+a[5]];
}
function rotation(node: Node): Matrix {
  const rad = node.rotationDeg * Math.PI / 180, c = Math.cos(rad), s = Math.sin(rad);
  const cx = node.bounds.x + node.bounds.width/2, cy = node.bounds.y + node.bounds.height/2;
  return [c,s,-s,c,cx-c*cx+s*cy,cy-s*cx-c*cy];
}
type Styles = NonNullable<DesignDocument['nodeStyles']>;
function transform(node:Node,styles:Styles): Matrix {
  return multiply(rotation(node),styles.find(s=>s.nodeId===node.id)?.affine as Matrix ?? identity);
}
function parentTransform(nodes:Node[],node:Node,styles:Styles): Matrix {
  const byId=new Map(nodes.map(n=>[n.id,n]));let result=identity;
  for(const id of [...ancestorIds(nodes,node.id)].reverse())result=multiply(result,transform(byId.get(id)!,styles));
  return result;
}
export function nodeFrame(nodes: Node[], node: Node, styles:Styles=[]): { x: number; y: number; a: number; b: number; c: number; d: number } {
  const byId = new Map(nodes.map(n => [n.id, n]));
  let matrix = identity;
  for (const id of [...ancestorIds(nodes,node.id)].reverse()) matrix = multiply(matrix,transform(byId.get(id)!,styles));
  matrix = multiply(matrix,node.type==='group' ? rotation(node) : transform(node,styles));
  return { x: matrix[0]*node.bounds.x+matrix[2]*node.bounds.y+matrix[4], y: matrix[1]*node.bounds.x+matrix[3]*node.bounds.y+matrix[5], a: matrix[0], b: matrix[1], c: matrix[2], d: matrix[3] };
}
export function groupAffine(before: Bounds, after: Bounds, affine: number[]=identity): Matrix {
  const sx=after.width/before.width,sy=after.height/before.height;
  return multiply([sx,0,0,sy,after.x-sx*before.x,after.y-sy*before.y],affine as Matrix);
}
export function frameDelta(frame:{a:number;b:number;c:number;d:number},dx:number,dy:number):{dx:number;dy:number} {
  const {a,b,c,d}=frame,det=a*d-b*c;
  return Math.abs(det)<1e-12 ? {dx:0,dy:0} : {dx:(d*dx-c*dy)/det,dy:(a*dy-b*dx)/det};
}
export function alignedNodes(nodes:Node[],selection:string[],direction:Parameters<typeof alignBounds>[1],styles:Styles=[]):{id:string;bounds:Bounds}[] {
  const chosen=nodes.filter(n=>selection.includes(n.id)&&!ancestorIds(nodes,n.id).some(id=>selection.includes(id)));
  const frames=chosen.map(n=>nodeFrame(nodes,n,styles));
  const bounds=chosen.map((n,i)=>{const f=frames[i],xs=[f.x,f.x+f.a*n.bounds.width,f.x+f.c*n.bounds.height,f.x+f.a*n.bounds.width+f.c*n.bounds.height],ys=[f.y,f.y+f.b*n.bounds.width,f.y+f.d*n.bounds.height,f.y+f.b*n.bounds.width+f.d*n.bounds.height];return {x:Math.min(...xs),y:Math.min(...ys),width:Math.max(...xs)-Math.min(...xs),height:Math.max(...ys)-Math.min(...ys)};});
  return alignBounds(bounds,direction).map((b,i)=>{const node=chosen[i],delta=localDrag(nodes,node,b.x-bounds[i].x,b.y-bounds[i].y,styles);return {id:node.id,bounds:{...node.bounds,x:node.bounds.x+delta.dx,y:node.bounds.y+delta.dy}};});
}
export function localDrag(nodes: Node[], node: Node, dx: number, dy: number, styles:Styles=[]): { dx: number; dy: number } {
  const [a,b,c,d]=parentTransform(nodes,node,styles),det=a*d-b*c;
  if(!Number.isFinite(det)||Math.abs(det)<1e-12)return {dx:0,dy:0};
  return { dx:(d*dx-c*dy)/det,dy:(a*dy-b*dx)/det };
}
export function resizeBounds(node: Node, dw: number, dh: number): Bounds {
  const width = Math.max(1,Math.min(16384,node.bounds.width+dw));
  const height = Math.max(1,Math.min(16384,node.bounds.height+dh));
  const xChange = width-node.bounds.width, yChange = height-node.bounds.height;
  const rad = node.rotationDeg*Math.PI/180,c=Math.cos(rad),s=Math.sin(rad);
  return { x:node.bounds.x+(c-1)*xChange/2-s*yChange/2,y:node.bounds.y+s*xChange/2+(c-1)*yChange/2,width,height };
}
export function alignBounds(bounds: Bounds[], direction: 'left' | 'center' | 'right' | 'top' | 'middle' | 'bottom'): Bounds[] {
  if (!bounds.length) return [];
  const l = Math.min(...bounds.map(b => b.x)), r = Math.max(...bounds.map(b => b.x + b.width));
  const t = Math.min(...bounds.map(b => b.y)), bottom = Math.max(...bounds.map(b => b.y + b.height));
  return bounds.map(b => ({ ...b,
    x: direction === 'left' ? l : direction === 'center' ? (l + r - b.width) / 2 : direction === 'right' ? r - b.width : b.x,
    y: direction === 'top' ? t : direction === 'middle' ? (t + bottom - b.height) / 2 : direction === 'bottom' ? bottom - b.height : b.y,
  }));
}
