// Mount only in a disposable RN/Expo app. The caller owns the exact build/target.
import React, {useEffect, useRef, useState} from 'react';
import {Button, NativeModules, ScrollView, Text, View} from 'react-native';
import {AcmeFaults} from './runtime.js';
export default function AcmeDiagnostics({corrected = false, uncaught = false, probe = null}) {
  const current = useRef(null);
  const [status, setStatus] = useState('Acme ready');
  const [frame, setFrame] = useState(0);
  useEffect(() => {
    const fixture = new AcmeFaults({corrected}); current.current = fixture;
    setFrame(0); setStatus('Acme ready');
    return () => { fixture.dispose(); if (current.current === fixture) current.current = null; };
  }, [corrected]);
  useEffect(() => { if (frame < 1) return; const timer = setTimeout(() => setFrame(frame - 1), 16); return () => clearTimeout(timer); }, [frame]);
  async function run(name) {
    const fixture = current.current; if (!fixture) return;
    console.info(`Acme trigger ${name} corrected=${corrected}`);
    NativeModules.AcmeBreadcrumbs?.mark(`Acme trigger ${name} corrected=${corrected}`);
    probe?.mark(`Acme ${name}`, `acme:${name}`, {corrected});
    if (name === 'reset') setFrame(0);
    if (name === 'exception' && uncaught) { console.info('Acme uncaught exception requested'); fixture.exception(); return; }
    const generation = fixture.generation;
    try { const value = await fixture[name](); if (!fixture.disposed && generation + (name === 'reset' ? 1 : 0) === fixture.generation) { setStatus(`${name}: ${JSON.stringify(value)}`); console.info(`Acme result ${name}: ${JSON.stringify(value)}`); } }
    catch (error) { if (!fixture.disposed && generation === fixture.generation) { setStatus(`${name}: ${error.message}`); console.error(`Acme caught ${name}: ${error.message}`); } }
  }
  return <ScrollView><Text>Acme diagnostics — {corrected ? 'corrected' : 'defective'}</Text>
    <Text testID="acme-result">{status}</Text><Text testID="acme-frame">{frame}</Text>
    <View style={{flexDirection: 'row', flexWrap: 'wrap'}}>
      {['exception', 'expensive', 'retain', 'bridge', 'network', 'reset'].map(name => <View key={name} style={{width: '50%'}}><Button title={name} accessibilityLabel={name} onPress={() => { void run(name); }} /></View>)}
    </View>
    <Button title="frame churn" onPress={() => { if (current.current) setFrame(current.current.churn()); }} />
  </ScrollView>;
}
