// Disposable app shell; instrumentation is opt-in through the prepared config.
import React, {Profiler, useEffect, useRef, useState} from 'react';
import {Button, DevSettings, NativeModules, SafeAreaView, Switch, Text} from 'react-native';
import AcmeDiagnostics from './AcmeDiagnostics.jsx';
import config from './acceptance.json';
const {installDiagnosticProbe} = require('./probe.cjs');

export default function DiagnosticApp() {
  const [corrected, setCorrected] = useState(false), [uncaught, setUncaught] = useState(false);
  const [probe, setProbe] = useState(null);
  const work = useRef(null);
  useEffect(() => () => clearTimeout(work.current), []);
  function profileWork() {
    clearTimeout(work.current); let remaining = 8;
    function acmeProfileWork() {
      const end = performance.now() + 100; let checksum = 0;
      while (performance.now() < end) checksum = (checksum + 1) >>> 0;
      console.info(`Acme profile work ${remaining} ${checksum}`);
      if (--remaining) work.current = setTimeout(acmeProfileWork, 40);
    }
    acmeProfileWork();
  }
  useEffect(() => {
    console.info('Acme React Native ready');
    NativeModules.AcmeBreadcrumbs?.mark('Acme native bridge ready');
    if (!config.enabled) return;
    const owned = installDiagnosticProbe(config.metadata);
    owned.mark('Acme mounted', 'acme:mount'); setProbe(owned);
    return () => { owned.uninstall(); };
  }, []);
  return <SafeAreaView style={{flex: 1, paddingTop: 32}}>
    <Text>Corrected mode</Text><Switch testID="acme-corrected" accessibilityLabel="Acme corrected mode" value={corrected} onValueChange={setCorrected} />
    <Text>Arm uncaught JavaScript error</Text><Switch testID="acme-uncaught" accessibilityLabel="Acme arm uncaught" value={uncaught} onValueChange={setUncaught} />
    <Text testID="acme-probe">{probe ? `Acme probe ${probe.snapshot().metadata.runtimeGeneration}` : 'Acme probe disabled'}</Text>
    <Text testID="acme-native-bridge">{NativeModules.AcmeBreadcrumbs ? 'Acme native bridge available' : 'Acme native bridge unavailable'}</Text>
    <Button title="Native breadcrumb" accessibilityLabel="Native breadcrumb" onPress={() => NativeModules.AcmeBreadcrumbs?.mark('Acme native bridge requested')} />
    <Button title="Reload JavaScript" accessibilityLabel="Reload JavaScript" onPress={() => DevSettings.reload()} />
    <Button title="Profile workload" accessibilityLabel="Profile workload" onPress={profileWork} />
    <Profiler id="Acme diagnostics" onRender={(...args) => probe?.onRender(...args)}>
      <AcmeDiagnostics corrected={corrected} uncaught={uncaught} probe={probe} />
    </Profiler>
  </SafeAreaView>;
}
