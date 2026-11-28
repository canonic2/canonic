import {AcmeFaults} from './runtime.js';

export function createController(schedule = requestAnimationFrame, cancel = cancelAnimationFrame) {
  let model = new AcmeFaults(), state = {status: 'Acme ready', frames: 0, corrected: false, armed: false}, timer = null, closed = false;
  const listeners = new Set();
  const emit = () => { if (!closed) for (const listener of listeners) listener({...state}); };
  const stop = () => { if (timer !== null) cancel(timer); timer = null; state.frames = 0; };
  return {
    subscribe(listener) { if (closed) throw Error('Acme controller disposed'); listeners.add(listener); listener({...state}); return () => listeners.delete(listener); },
    corrected(value) { if (closed) return; stop(); model.dispose(); model = new AcmeFaults({corrected: value}); state = {status: 'Acme ready', frames: 0, corrected: value, armed: false}; emit(); },
    arm(value) { if (!closed) { state.armed = value; emit(); } },
    async run(name) {
      if (closed) return;
      if (name === 'churn') {
        stop(); state.frames = model.churn(); emit();
        const tick = () => { timer = null; if (closed) return; state.frames--; emit(); if (state.frames > 0) timer = schedule(tick); };
        if (!closed && state.frames > 0) timer = schedule(tick); return;
      }
      if (name === 'reset') { stop(); model.reset(); state.armed = false; state.status = 'Acme reset'; emit(); return; }
      if (name === 'uncaught') {
        if (!state.armed) { state.status = 'Arm this app first'; emit(); return; }
        state.armed = false; emit(); const owner = model;
        void Promise.resolve().then(() => { if (!closed && !owner.disposed) owner.exception(); }); return;
      }
      const owner = model, generation = model.generation;
      try {
        const value = await owner[name]();
        if (!closed && !owner.disposed && owner.generation === generation) { state.status = `${name}: ${JSON.stringify(value)}`; emit(); }
      } catch (error) {
        if (!closed && !owner.disposed && owner.generation === generation) { state.status = `${name}: ${error.message}`; emit(); }
      }
    },
    dispose() { if (!closed) { stop(); closed = true; model.dispose(); listeners.clear(); } },
  };
}

// Each framework subscribes into its own component state and owns this cleanup.
export function mountControls(root, onChange) {
  const controller = createController();
  function toggle(title, action) {
    const label = document.createElement('label'), input = document.createElement('input'); input.type = 'checkbox';
    input.onchange = () => action(input.checked); label.append(input, title); root.append(label, document.createElement('br')); return input;
  }
  const corrected = toggle('Corrected mode', value => controller.corrected(value));
  const armed = toggle('Arm uncaught JavaScript error', value => controller.arm(value));
  for (const [name, title] of [['exception','Caught exception'],['expensive','Bounded CPU work'],['retain','Retain one MiB'],['churn','Frame-state churn'],['bridge','Simulated bridge failure'],['network','Simulated network failure'],['reset','Reset'],['uncaught','Uncaught JavaScript error']]) {
    const button = document.createElement('button'); button.textContent = title; button.onclick = () => { void controller.run(name); }; root.append(button, document.createElement('br'));
  }
  controller.subscribe(state => { corrected.checked = state.corrected; armed.checked = state.armed; onChange(state); });
  return () => { controller.dispose(); root.replaceChildren(); };
}
