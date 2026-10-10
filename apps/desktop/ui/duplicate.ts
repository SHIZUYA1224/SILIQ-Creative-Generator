import type { DesignDocument, EditCommand, Node } from './api';
import { ancestorIds } from './geometry';
import { parsePathPoints,segmentPoints,serializePathPoints } from './path-points';

type Style = NonNullable<DesignDocument['nodeStyles']>[number];
const identity = [1, 0, 0, 1, 0, 0];

// Group children keep their absolute source coordinates. Leaf bounds move,
// so an existing leaf affine is conjugated by the same translation instead.
function movedAffine(affine: number[] | undefined, group: boolean, dx: number, dy: number): number[] {
  const [a,b,c,d,e,f] = affine ?? identity;
  return [a,b,c,d,e+dx-(group ? 0 : a*dx+c*dy),f+dy-(group ? 0 : b*dx+d*dy)];
}
function movedPath(data:string):string {
  const segments=parsePathPoints(data);
  for (const segment of segments) for (const [,point] of segmentPoints(segment)) { point.x+=20;point.y+=20; }
  return serializePathPoints(segments);
}

export function duplicateSelection(document: DesignDocument, selected: string[], makeId = () => `copy-${crypto.randomUUID().slice(0,8)}`): { command: EditCommand; selected: string[] } | null {
  const byId = new Map(document.nodes.map(node => [node.id,node]));
  const top = [...new Set(selected)].filter(id => id !== document.baseArtboard.rootNodeId && byId.has(id) && !ancestorIds(document.nodes,id).some(parent => selected.includes(parent)));
  if (!top.length) return null;
  const parentOf = new Map(document.nodes.flatMap(node => node.type === 'group' ? node.childIds.map(id => [id,node.id] as const) : []));
  const source: Node[] = [];
  const collect = (id: string) => {
    const node = byId.get(id)!; source.push(node);
    if (node.type === 'group') node.childIds.forEach(collect);
  };
  top.forEach(collect);
  if (document.nodes.length + source.length > 2000) throw new Error('複製するとレイヤー上限の 2,000 件を超えます。');
  const idMap = new Map(source.map(node => [node.id,makeId()]));
  if (new Set(idMap.values()).size !== idMap.size || [...idMap.values()].some(id => byId.has(id))) throw new Error('複製のレイヤー ID が重複しています。');
  const commands: EditCommand[] = [];
  const children = new Map<string,string[]>(document.nodes.flatMap(node => node.type === 'group' ? [[node.id,[...node.childIds]] as [string,string[]]] : []));
  const create = (id: string, parentId: string, index: number, moved: boolean) => {
    const original = byId.get(id)!;
    const copy = structuredClone(original);
    copy.id = idMap.get(id)!; copy.name = `${copy.name} コピー`; copy.locked = false;
    if (moved) copy.bounds = { ...copy.bounds,x:copy.bounds.x+20,y:copy.bounds.y+20 };
    if (copy.type === 'group') copy.childIds = [];
    commands.push({ op:'addNode',parentId,index,node:copy });
    if (original.type === 'group') original.childIds.forEach((child,i) => create(child,copy.id,i,false));
  };
  for (const id of top) {
    const parentId = parentOf.get(id)!;
    const siblings = children.get(parentId)!;
    const index = siblings.indexOf(id)+1;
    create(id,parentId,index,true);
    siblings.splice(index,0,idMap.get(id)!);
  }
  const originals = new Map((document.nodeStyles ?? []).map(style => [style.nodeId,style]));
  for (const node of source) {
    const original = originals.get(node.id);
    if (!original && !(top.includes(node.id) && node.type === 'group')) continue;
    const style: Style = { ...structuredClone(original ?? { nodeId:node.id }),nodeId:idMap.get(node.id)! };
    if (top.includes(node.id) && (node.type === 'group' || original?.affine)) style.affine = movedAffine(original?.affine,node.type === 'group',20,20);
    if (top.includes(node.id) && node.type !== 'group') {
      if (style.clip) style.clip.data=movedPath(style.clip.data);
      if (style.mask) style.mask.data=movedPath(style.mask.data);
    }
    if (style.protectedFrom) style.protectedFrom = style.protectedFrom.map(id => idMap.get(id) ?? id);
    commands.push({ op:'setNodeStyle',nodeId:style.nodeId,style });
  }
  // Preserve each size's explicit layout and fit choices without flattening
  // the active variant into the canonical nodes.
  for (const variant of document.variants) {
    const patches = variant.nodePatches.filter(patch => idMap.has(patch.nodeId)).map(patch => {
      const copy = structuredClone(patch);
      copy.nodeId = idMap.get(patch.nodeId)!;
      if (top.includes(patch.nodeId)) {
        copy.bounds = { ...copy.bounds,x:copy.bounds.x+20,y:copy.bounds.y+20 };
        if (byId.get(patch.nodeId)?.type === 'group') copy.affine = movedAffine(patch.affine ?? originals.get(patch.nodeId)?.affine,true,20,20);
      }
      return copy;
    });
    if (patches.length) commands.push({ op:'setVariant',variant:{ ...variant,approvalStatus:'draft',nodePatches:[...variant.nodePatches,...patches] } });
  }
  // Set locks from the leaves upward after styles/patches are installed.
  for (const node of [...source].reverse()) if (node.locked) commands.push({ op:'setLocked',nodeId:idMap.get(node.id)!,locked:true });
  return { command:{ op:'batch',commands },selected:top.map(id => idMap.get(id)!) };
}
