'use client';
import {useEffect, useRef, useState} from 'react';
import {mountControls} from '../src/controls.js';
export default function App() {
  const root = useRef(null), [state, setState] = useState({});
  useEffect(() => mountControls(root.current, setState), []);
  return <section><h1>Acme Next.js</h1><div ref={root} /><output>{JSON.stringify(state)}</output></section>;
}
