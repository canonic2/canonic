/* The Lucide icons the design examples draw, imported one file each and set
   where workbench/icons.js looks for them. The whole set would be compiled
   into every docs page's examples, with a multi-megabyte source map; these
   are the sample sizes' and spaces' icons, the components' own, and a few
   common size icons to type into Custom size… and Edit sizes…. */

import ArrowDown from 'lucide/dist/esm/icons/arrow-down.mjs';
import ArrowUp from 'lucide/dist/esm/icons/arrow-up.mjs';
import Check from 'lucide/dist/esm/icons/check.mjs';
import ChevronDown from 'lucide/dist/esm/icons/chevron-down.mjs';
import ChevronsUpDown from 'lucide/dist/esm/icons/chevrons-up-down.mjs';
import Component from 'lucide/dist/esm/icons/component.mjs';
import FileText from 'lucide/dist/esm/icons/file-text.mjs';
import Frame from 'lucide/dist/esm/icons/frame.mjs';
import Hammer from 'lucide/dist/esm/icons/hammer.mjs';
import Laptop from 'lucide/dist/esm/icons/laptop.mjs';
import Maximize from 'lucide/dist/esm/icons/maximize.mjs';
import Minimize2 from 'lucide/dist/esm/icons/minimize-2.mjs';
import Monitor from 'lucide/dist/esm/icons/monitor.mjs';
import MousePointerClick from 'lucide/dist/esm/icons/mouse-pointer-click.mjs';
import Palette from 'lucide/dist/esm/icons/palette.mjs';
import PanelBottom from 'lucide/dist/esm/icons/panel-bottom.mjs';
import PanelLeft from 'lucide/dist/esm/icons/panel-left.mjs';
import PanelRight from 'lucide/dist/esm/icons/panel-right.mjs';
import PanelTop from 'lucide/dist/esm/icons/panel-top.mjs';
import Plus from 'lucide/dist/esm/icons/plus.mjs';
import Scaling from 'lucide/dist/esm/icons/scaling.mjs';
import Smartphone from 'lucide/dist/esm/icons/smartphone.mjs';
import Tablet from 'lucide/dist/esm/icons/tablet.mjs';
import Trash2 from 'lucide/dist/esm/icons/trash-2.mjs';
import Tv from 'lucide/dist/esm/icons/tv.mjs';
import X from 'lucide/dist/esm/icons/x.mjs';
import '../workbench/icons.js';

type IconNode = [string, Record<string, string>][];

const icons: Record<string, IconNode> = {
  ArrowDown, ArrowUp, Check, ChevronDown, ChevronsUpDown, Component, FileText, Frame, Hammer, Laptop,
  Maximize, Minimize2, Monitor, MousePointerClick, Palette, PanelBottom, PanelLeft, PanelRight, PanelTop,
  Plus, Scaling, Smartphone, Tablet, Trash2, Tv, X,
};

const host = window as unknown as { lucide?: { icons: Record<string, IconNode> }; wbIcon: (name: string, size?: number) => Element };
host.lucide ??= { icons };

/** The canvas's icon renderer, drawing from the icons above. */
export const icon = host.wbIcon;
