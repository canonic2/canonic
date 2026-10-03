export default function mount(canvas, context) {
  const gallery = document.createElement('div');
  gallery.className = 'gallery';
  const button = document.createElement('button');
  button.className = 'btn ' + (context.inputs.variant === 'primary' ? '' : context.inputs.variant);
  button.textContent = context.inputs.label;
  button.disabled = context.inputs.disabled;
  button.addEventListener('click', () => context.action('click', context.inputs.label), { signal: context.signal });
  gallery.append(button);
  canvas.append(gallery);
}
