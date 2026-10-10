import { invoke, isTauri } from '@tauri-apps/api/core';
import type { DesignDocument } from '../../../packages/contracts-generated/design';

export type { DesignDocument, Node, Bounds, Paint, Variant, Slot } from '../../../packages/contracts-generated/design';
export interface Issue {
  severity: 'error' | 'warning';
  code: string;
  message: string;
  location: { nodeId?: string; variantId?: string; rowId?: string; assetId?: string; fontId?: string };
  remedy: string;
}
export interface SessionView {
  sessionId: string;
  document: DesignDocument;
  mode: 'authoring' | 'production';
  canUndo: boolean;
  canRedo: boolean;
  issues: Issue[];
}
export interface PreviewMetadata {
  documentRevision: number;
  requestId: number;
  width: number;
  height: number;
  stride?: number;
  alphaType?: 'straight';
  frameSha256?: string;
  payloadLength: number;
  pixelFormat: 'PNG' | 'png' | 'RGBA8';
  colorSpace: 'sRGB';
  quality: 'exact';
  outputSha256: string;
  issues: Issue[];
}
export type EditCommand = { op: string; [key: string]: unknown };
export const nativeAvailable = isTauri();
export async function call<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  if (!nativeAvailable) throw new Error('デスクトップアプリから開いてください。');
  return invoke<T>(command, args);
}
export function issuesFrom(error: unknown): Issue[] {
  if (Array.isArray(error)) return error as Issue[];
  if (error && typeof error === 'object' && 'issues' in error && Array.isArray(error.issues)) return error.issues as Issue[];
  return [{ severity: 'error', code: 'E_OPERATION', message: String(error instanceof Error ? error.message : error), location: {}, remedy: '操作内容を確認して、もう一度お試しください。' }];
}
export const transactionId = () => `edit-${crypto.randomUUID()}`;
