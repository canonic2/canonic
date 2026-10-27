/** The canvas dot grid: one full-screen WebGPU pass drawn beneath every canvas surface. */

const shader = `
struct Grid { camera: vec4f, background: vec4f, dot: vec4f };
@group(0) @binding(0) var<uniform> grid: Grid;
@vertex fn vertex(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
  var points=array<vec2f,3>(vec2f(-1,-1),vec2f(3,-1),vec2f(-1,3)); return vec4f(points[i],0,1);
}
@fragment fn fragment(@builtin(position) point: vec4f) -> @location(0) vec4f {
  let spacing=grid.camera.z;let phase=(point.xy-grid.camera.xy)/spacing;
  let distance=abs(fract(phase+0.5)-0.5)*spacing;
  return select(grid.background,grid.dot,max(distance.x,distance.y)<grid.camera.w);
}`;

/** Dot spacing in CSS pixels: 24 world pixels, doubled while it would fall below 12 on screen. */
export function gridSpacing(scale) {
  let spacing = 24 * scale;
  while (spacing < 12) spacing *= 2;
  return spacing;
}

/** Uniform values for a camera ({x, y, scale} in CSS pixels) at a device pixel ratio.
 *  `background` and `line` are RGBA in 0–1; dots use the line colour at half intensity. */
export function gridUniforms(camera, dpr, background, line) {
  return new Float32Array([
    camera.x * dpr, camera.y * dpr, gridSpacing(camera.scale) * dpr, Math.max(0.7, dpr * 0.5),
    ...background, ...line.map((c, i) => i === 3 ? 1 : c * 0.5),
  ]);
}

/** Grid pipeline for one device and surface format. Call `draw` first in a render pass. */
export function createGrid(device, format) {
  const module = device.createShaderModule({ code: shader });
  const pipeline = device.createRenderPipeline({ layout: 'auto', vertex: { module, entryPoint: 'vertex' },
    fragment: { module, entryPoint: 'fragment', targets: [{ format }] }, primitive: { topology: 'triangle-list' } });
  const buffer = device.createBuffer({ size: 48, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  const group = device.createBindGroup({ layout: pipeline.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer } }] });
  return {
    draw(pass, camera, dpr, background, line) {
      device.queue.writeBuffer(buffer, 0, gridUniforms(camera, dpr, background, line));
      pass.setPipeline(pipeline); pass.setBindGroup(0, group); pass.draw(3);
    },
    dispose() { buffer.destroy(); },
  };
}
