import { describe, expect, it } from 'vitest';
import { alignBounds, ancestorIds, drawNodes, isEffectivelyLocked, localDrag, nodeFrame, resizeBounds, resolvedNodes } from './geometry';
import template from '../../../fixtures/public-safe/documents/basic.json';
import type { DesignDocument } from './api';

const doc = template as DesignDocument;
describe('editor geometry and author overrides', () => {
  it('uses the selected variant without changing the canonical document', () => {
    const original = JSON.stringify(doc);
    const portrait = resolvedNodes(doc, 'portrait');
    expect(portrait.find(n => n.id === 'headline')!.bounds).toEqual(doc.variants[1].nodePatches.find(p => p.nodeId === 'headline')!.bounds);
    expect(JSON.stringify(doc)).toBe(original);
  });
  it('inherits group locks instead of letting children bypass them', () => {
    const nodes = structuredClone(doc.nodes);
    const group = nodes.find(n => n.id === 'accent-group')!; group.locked = true;
    expect(ancestorIds(nodes, 'accent')).toEqual(['accent-group', 'root']);
    expect(isEffectivelyLocked(nodes, nodes.find(n => n.id === 'accent')!)).toBe(true);
  });
  it('aligns mixed widths to their shared right edge, retaining their sizes', () => {
    expect(alignBounds([{ x: 5, y: 10, width: 80, height: 30 }, { x: 150, y: 20, width: 20, height: 60 }], 'right')).toEqual([{ x: 90, y: 10, width: 80, height: 30 }, { x: 150, y: 20, width: 20, height: 60 }]);
  });
  it('follows child draw order and hides descendants of hidden groups', () => {
    const nodes = structuredClone(doc.nodes);
    const root = nodes.find(n => n.type === 'group' && n.id === 'root')!;
    if (root.type !== 'group') throw new Error('fixture root');
    root.childIds.reverse();
    expect(drawNodes(nodes,'root')[0].id).toBe('brand-logo');
    nodes.find(n => n.id === 'accent-group')!.visible = false;
    expect(drawNodes(nodes,'root').some(n => n.id === 'accent')).toBe(false);
  });
  it('maps parent rotations into the overlay and back into canonical drag coordinates', () => {
    const nodes = structuredClone(doc.nodes);
    const group = nodes.find(n => n.id === 'accent-group')!; group.rotationDeg = 90;
    const child = nodes.find(n => n.id === 'accent')!; child.rotationDeg = 0;
    const frame = nodeFrame(nodes,child);
    expect(frame.a).toBeCloseTo(0); expect(frame.b).toBeCloseTo(1);
    expect(localDrag(nodes,child,0,20).dx).toBeCloseTo(20);
    expect(localDrag(nodes,child,0,20).dy).toBeCloseTo(0);
  });
  it('keeps the opposite corner fixed when resizing a rotated node', () => {
    const nodes=structuredClone(doc.nodes),node=nodes.find(n=>n.id==='photo')!;node.rotationDeg=60;
    const before=nodeFrame(nodes,node),after=nodeFrame(nodes,{...node,bounds:resizeBounds(node,100,80)});
    expect(after.x).toBeCloseTo(before.x);expect(after.y).toBeCloseTo(before.y);
    expect(resizeBounds(node,-99999,-99999).width).toBe(1);
  });
});

describe('group scaling',()=>{
  it('uses a group affine once for children and not a second time for its group bounds',()=>{
    const nodes=structuredClone(doc.nodes),group=nodes.find(n=>n.id==='accent-group')!,child=nodes.find(n=>n.id==='accent')!;
    group.rotationDeg=0;child.rotationDeg=0;
    const styles=[{nodeId:group.id,blendMode:'normal' as const,effects:[],affine:[2,0,0,3,15,25]}];
    const frame=nodeFrame(nodes,child,styles);
    expect(frame.x).toBe(child.bounds.x*2+15);expect(frame.y).toBe(child.bounds.y*3+25);
    expect(nodeFrame(nodes,group,styles).x).toBe(group.bounds.x);
    expect(localDrag(nodes,child,20,30,styles)).toEqual({dx:10,dy:10});
  });
});
