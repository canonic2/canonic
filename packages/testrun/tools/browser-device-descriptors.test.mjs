import assert from 'node:assert/strict';
import test from 'node:test';
import { descriptors } from './browser-device-descriptors.mjs';

const section = body => `// packages/isomorphic/deviceDescriptorsSource.json
var deviceDescriptorsSource_default;
var init_deviceDescriptorsSource = __esm({
  "packages/isomorphic/deviceDescriptorsSource.json"() {
    deviceDescriptorsSource_default = {
${body}
    };
  }
});
`;
const acme = `      "Acme Phone": {
        userAgent: "Mozilla/5.0 (Linux; Android 14) Acme",
        viewport: {
          width: 390,
          height: 844
        },
        deviceScaleFactor: 3,
        isMobile: true,
        hasTouch: true,
        defaultBrowserType: "chromium"
      }`;

test('extracts the literal table in upstream order without evaluating it', () => {
  const desktop = `      "Acme Desktop": {
        userAgent: "Acme/1.0 (Windows NT 10.0)",
        screen: {
          width: 1920,
          height: 1080
        },
        viewport: {
          width: 1280,
          height: 720
        },
        deviceScaleFactor: 1,
        isMobile: false,
        hasTouch: false,
        defaultBrowserType: "firefox"
      }`;
  const table = JSON.parse(descriptors(section(`${acme},\n${desktop}`)));
  assert.deepEqual(Object.keys(table), ['Acme Phone', 'Acme Desktop']);
  assert.equal(table['Acme Phone'].viewport.height, 844);
  assert.deepEqual(table['Acme Desktop'].screen, {width: 1920, height: 1080});
});

test('refuses missing tables, unknown fields and invalid sizes', () => {
  assert.throws(() => descriptors('var other = {};'), /Missing pinned device descriptor table/);
  assert.throws(() => descriptors(section(acme.replace('hasTouch: true', 'hasTouch: true,\n        locale: "en-US"'))), /Invalid device descriptor/);
  assert.throws(() => descriptors(section(acme.replace('width: 390', 'width: 0'))), /Invalid Acme Phone viewport size/);
  assert.throws(() => descriptors(section(acme.replace('"chromium"', '"safari"'))), /Invalid device descriptor/);
});
