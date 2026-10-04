import type { Artboard } from './model.ts';
export interface Camera { x: number; y: number; scale: number }
export function bounds(boards: readonly Artboard[]) {
  const width = Math.max(1, ...boards.map(b => b.x + b.size.width));
  const height = Math.max(1, ...boards.map(b => b.y + b.size.height));
  return { width, height };
}
export function fit(boards: readonly Artboard[], viewport: { width: number; height: number }): Camera {
  if (!boards.length) return { x: 32, y: 32, scale: 1 };
  const minX = Math.min(...boards.map(b => b.x));
  const minY = Math.min(...boards.map(b => b.y - 32));
  const box = bounds(boards);
  const width = box.width - minX, height = box.height - minY;
  const scale = Math.max(0.0001, Math.min(1, (viewport.width - 64) / width, (viewport.height - 64) / height));
  return { x: (viewport.width - width * scale) / 2 - minX * scale, y: (viewport.height - height * scale) / 2 - minY * scale, scale };
}
export function zoom(camera: Camera, scale: number, point: { x: number; y: number }): Camera {
  const next = Math.max(0.0001, Math.min(8, scale));
  return { scale: next, x: point.x - (point.x - camera.x) * next / camera.scale, y: point.y - (point.y - camera.y) * next / camera.scale };
}
