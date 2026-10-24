export const styles = `
      :host{display:block;position:relative;overflow:hidden;min-width:0;min-height:0;background:transparent;color:var(--wb-fg);outline:none;touch-action:none}
      .world{position:absolute;inset:0 auto auto 0;transform-origin:0 0}
      .staging{position:absolute;inset:0;visibility:hidden;pointer-events:none}
      .staging>div{position:absolute;left:0;top:0}
      section{position:absolute;box-shadow:0 0 0 1px var(--wb-line);background:var(--wb-raised)}
      section[data-content-fitted=true]{background:transparent;box-shadow:none}
      section[data-selected=true]{box-shadow:0 0 0 2px var(--wb-accent)}
      button{position:absolute;bottom:100%;left:0;width:100%;height:calc(28px / var(--camera-scale,1));padding:0;border:0;background:transparent;color:var(--wb-fg-2);font:calc(12px / var(--camera-scale,1)) var(--wb-ui-font);text-align:left;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;cursor:pointer}
      button:focus-visible{outline:2px solid var(--wb-focus)}
      section[data-selected=true] button{color:var(--wb-fg)}
      ::slotted(iframe){display:block;width:100%;height:100%;border:0}
      p{position:absolute;inset:40% 0 auto;text-align:center;color:var(--wb-fg-2);font:14px var(--wb-ui-font)}
      .pan{position:absolute;inset:0;z-index:10;cursor:grabbing}
      .pan[hidden]{display:none}
      .rail{position:absolute;z-index:2;display:none}
      section[data-selected=true][data-resizable=true] .rail{display:block}
      .rail[data-edge=left]{left:-5px;top:0;bottom:0;width:5px;cursor:ew-resize}
      .rail[data-edge=right]{right:-5px;top:0;bottom:0;width:5px;cursor:ew-resize}
      .rail[data-edge=bottom]{left:0;right:0;bottom:-5px;height:5px;cursor:ns-resize}
      .rail[data-edge=bottom-left]{left:-5px;bottom:-5px;width:10px;height:10px;cursor:nesw-resize}
      .rail[data-edge=bottom-right]{right:-5px;bottom:-5px;width:10px;height:10px;cursor:nwse-resize}
      :host([reading]){background:white}
      :host([reading]) section{box-shadow:none;background:white}
      :host([reading]) button,:host([reading]) .rail{display:none}
    `;
