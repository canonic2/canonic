import React from 'react';
import { createRoot } from 'react-dom/client';
export async function mount(canvas, Component, context, environment) {
  let tree = React.createElement(Component, context.inputs);
  if (environment?.wrap) tree = environment.wrap(tree, context);
  const root = createRoot(canvas);
  let disposed = false;
  let mounted = false;
  const unmount = () => { if (!disposed) { disposed = true; root.unmount(); } };
  return new Promise((resolve, reject) => {
    class Boundary extends React.Component {
      constructor(props) { super(props); this.state = { failed: false }; }
      static getDerivedStateFromError() { return { failed: true }; }
      componentDidCatch(error) {
        if (mounted) context.error(error); else reject(error);
        queueMicrotask(unmount);
      }
      render() { return this.state.failed ? null : this.props.children; }
    }
    function Ready() {
      React.useLayoutEffect(() => { mounted = true; resolve(unmount); }, []);
      return tree;
    }
    root.render(React.createElement(Boundary, null, React.createElement(Ready)));
  });
}
