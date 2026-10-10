import { cleanup, fireEvent, render, screen } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import type { Node } from './api';
import { PathEditor } from './PathEditor';
afterEach(cleanup);
const node: Extract<Node, { type: 'path' }> = { id: 'curve', name: 'curve', type: 'path', bounds: { x: 0, y: 0, width: 100, height: 100 }, rotationDeg: 0, opacity: 1, visible: true, locked: false, data: 'M0 0 C10 20 30 40 50 60 Z', viewBox: { x: 0, y: 0, width: 100, height: 100 }, fill: { kind: 'solid', color: '#ff0000' }, fillRule: 'non-zero' };
it('commits a numeric handle edit as one typed native command', () => {
  const edit = vi.fn(); render(<PathEditor node={node} disabled={false} edit={edit} />);
  const input = screen.getByLabelText('点 2 ハンドル 2 Y'); fireEvent.change(input, { target: { value: '45' } }); fireEvent.blur(input);
  expect(edit).toHaveBeenCalledTimes(1); expect(edit).toHaveBeenCalledWith({ op: 'setPathData', nodeId: 'curve', data: 'M0 0 C10 20 30 45 50 60 Z' });
});
it('preserves unsupported raw data and reports the failure', () => {
  const edit = vi.fn();const raw = 'M0 0 A1 2 0 0 0 3 4';render(<PathEditor node={{ ...node, data: raw }} disabled={false} edit={edit} />);
  expect((screen.getByLabelText('輪郭データ') as HTMLTextAreaElement).value).toBe(raw);
  expect(screen.getByRole('alert').textContent).toContain('元データは保持');expect((screen.getByRole('button', { name: 'パスデータを反映' }) as HTMLButtonElement).disabled).toBe(true);expect(edit).not.toHaveBeenCalled();
});
