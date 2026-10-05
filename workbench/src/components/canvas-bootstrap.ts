/* The canvas's components: the sidebar's space controls, the size switcher
   with its dialogs, and the Actions switch. The sidebar webview bundles
   bootstrap.ts alone. */
import { registerActionsComponents, registerSizeComponents, registerSpaceComponents } from './register.ts';

registerSpaceComponents();
registerSizeComponents();
registerActionsComponents();
