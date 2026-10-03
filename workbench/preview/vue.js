import { createApp, h } from 'vue';
export async function mount(canvas, Component, context, environment) {
  const tree = { render: () => environment?.wrap
    ? environment.wrap(h(Component, context.inputs), context)
    : h(Component, context.inputs) };
  const app = createApp(tree);
  let failure;
  let mounted = false;
  if (environment?.configure) await environment.configure(app, context);
  const errorHandler = app.config.errorHandler;
  app.config.errorHandler = (...args) => {
    if (mounted) context.error(args[0]); else failure = args[0];
    if (errorHandler) errorHandler(...args);
  };
  try { app.mount(canvas); } catch (error) { app.unmount(); throw error; }
  if (failure) { app.unmount(); throw failure; }
  mounted = true;
  return () => app.unmount();
}
