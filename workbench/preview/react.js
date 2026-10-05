import React from 'react';
import { createRoot } from 'react-dom/client';

// Both preview frames and docs example stages use this native host contract.
export async function mountNative(canvas, Component, context, environment) {
  const hadClass = canvas.classList.contains('wb-react-native-root');
  canvas.classList.add('wb-react-native-root');
  const restore = () => { if (!hadClass) canvas.classList.remove('wb-react-native-root'); };
  try {
    const unmount = await mount(canvas, Component, context, environment);
    return () => { try { unmount(); } finally { restore(); } };
  } catch (error) {
    restore();
    throw error;
  }
}

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
