import '@angular/compiler';
import {Component, signal, provideZonelessChangeDetection} from '@angular/core';
import {bootstrapApplication} from '@angular/platform-browser';
import {mountControls} from './controls.js';
class AcmeApp {
  state = signal('{}');
  ngAfterViewInit() { queueMicrotask(() => { if (!this.closed) this.dispose = mountControls(document.querySelector('#controls'), state => this.state.set(JSON.stringify(state))); }); }
  ngOnDestroy() { this.closed = true; this.dispose?.(); }
}
Component({selector: 'acme-diagnostics', standalone: true, template: '<h1>Acme Angular JIT</h1><div id="controls"></div><output>{{state()}}</output>'})(AcmeApp);
document.querySelector('#app').append(document.createElement('acme-diagnostics'));
const app = bootstrapApplication(AcmeApp, {providers: [provideZonelessChangeDetection()]});
if (import.meta.hot) import.meta.hot.dispose(() => { void app.then(value => value.destroy()); });
