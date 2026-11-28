import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {join, resolve} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';

const run = promisify(execFile);
const root = resolve(import.meta.dirname, '..');
const native = join(root, 'tests/fixtures/diagnostics');
async function fixture(compile, source, extension) {
  assert.equal(process.env.DIAGNOSTIC_HOST, 'macos', 'Run with DIAGNOSTIC_HOST=macos to select these host-only fixtures');
  assert.equal(process.platform, 'darwin', 'This exact native host fixture requires macOS/Xcode; do not infer another host');
  const directory = await mkdtemp(join(tmpdir(), 'testrun-diagnostic-native-'));
  try {
    const driver = join(directory, `main.${extension}`); const binary = join(directory, 'acme-fixture');
    await writeFile(driver, source);
    await run('/usr/bin/xcrun', [...compile, driver, '-o', binary], {cwd: root, timeout: 30000, maxBuffer: 1048576});
    const result = await run(binary, [], {cwd: directory, timeout: 3000, maxBuffer: 65536});
    assert.equal(result.stdout.trim(), 'Acme fixture ready');
  } finally { await rm(directory, {recursive: true, force: true}); }
}
test('Acme C++ host control retains/replaces/releases one MiB without fatal or hang calls', async () => {
  await fixture(['clang++', '-std=c++17', join(native, 'native/acme_faults.cpp')], `
#include <cassert>
#include <cstdio>
extern "C" void acme_native_crash(bool);
extern "C" void acme_native_hang(bool);
extern "C" int acme_native_retain(bool);
int main() { acme_native_crash(true); acme_native_hang(true); for(int n=0;n<3;n++) assert(acme_native_retain(false)==1048576); assert(acme_native_retain(true)==0); puts("Acme fixture ready"); }
`, 'cpp');
});
test('Acme Swift host control releases bounded Data under main-actor ownership', async () => {
  await fixture(['swiftc', '-parse-as-library', '-swift-version', '6', join(native, 'apple/AcmeSwiftFaults.swift')], `
import Foundation
@main struct Driver {
  @MainActor static func main() { AcmeSwiftFaults.crash(corrected:true); AcmeSwiftFaults.mainThreadHang(corrected:true); AcmeSwiftFaults.backgroundFailure(corrected:true); for _ in 0..<3 { precondition(AcmeSwiftFaults.retain(corrected:false)==1048576) }; AcmeSwiftFaults.reset(); precondition(AcmeSwiftFaults.retain(corrected:true)==0); print("Acme fixture ready") }
}
`, 'swift');
});
test('Acme Objective-C host exception is observed and corrected without an uncaught native crash', async () => {
  await fixture(['clang', '-fobjc-arc', '-framework', 'Foundation', '-I', join(native, 'apple'), join(native, 'apple/AcmeObjCFaults.m')], `
#import "AcmeObjCFaults.h"
#include <assert.h>
#include <stdio.h>
int main() { @autoreleasepool { BOOL caught=NO; @try { [AcmeObjCFaults exceptionCorrected:NO]; } @catch(NSException *e) { caught=[e.name isEqualToString:@"AcmeFixtureException"]; } assert(caught); [AcmeObjCFaults exceptionCorrected:YES]; [AcmeObjCFaults mainThreadHangCorrected:YES]; [AcmeObjCFaults backgroundFailureCorrected:YES]; puts("Acme fixture ready"); } }
`, 'm');
});
