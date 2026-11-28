import test from 'node:test';
import assert from 'node:assert/strict';
import {mkdtemp, rm, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {delimiter, dirname, isAbsolute, join, resolve} from 'node:path';
import {execFile} from 'node:child_process';
import {promisify} from 'node:util';

const run = promisify(execFile);
test('Acme Java/Kotlin compile against the selected Android API and expose caught/control faults on the host JVM', async () => {
  const jar = process.env.DIAGNOSTIC_ANDROID_JAR; const javac = process.env.DIAGNOSTIC_JAVAC; const kotlinc = process.env.DIAGNOSTIC_KOTLINC;
  for (const value of [jar, javac, kotlinc]) assert.ok(value && isAbsolute(value), 'Select installed absolute DIAGNOSTIC_ANDROID_JAR, DIAGNOSTIC_JAVAC and DIAGNOSTIC_KOTLINC paths');
  const directory = await mkdtemp(join(tmpdir(), 'testrun-diagnostic-android-'));
  const source = resolve(import.meta.dirname, '../tests/fixtures/diagnostics/android');
  const options = {timeout: 30000, maxBuffer: 1048576, cwd: directory};
  try {
    const driver = join(directory, 'Driver.java');
    await writeFile(driver, `import com.example.acme.diagnostics.AcmeFaults;
public class Driver { public static void main(String[] args) {
 boolean caught=false; try { AcmeFaults.exception(false); } catch(IllegalStateException e) { caught=e.getMessage().equals("Acme Java fixture failure"); }
 if(!caught || !AcmeFaults.exception(true).equals("ready")) throw new AssertionError("Java exception contract");
 for(int n=0;n<3;n++) if(AcmeFaults.retain(false)!=1048576) throw new AssertionError("retention bound");
 AcmeFaults.reset(); if(AcmeFaults.retain(true)!=0) throw new AssertionError("release");
 AcmeFaults.mainThreadHang(true); AcmeFaults.backgroundFailure(true); AcmeFaults.killSelf(true);
 System.out.println("Acme Java ready"); }}
`);
    await run(javac, ['-classpath', jar, '-d', directory, join(source, 'AcmeFaults.java'), driver], options);
    const java = await run(join(dirname(javac), 'java'), ['-classpath', `${directory}${delimiter}${jar}`, 'Driver'], options);
    assert.equal(java.stdout.trim(), 'Acme Java ready');
    const kotlinDriver = join(directory, 'Driver.kt');
    await writeFile(kotlinDriver, `import com.example.acme.diagnostics.AcmeKotlinFaults
fun main() { var caught=false; try { AcmeKotlinFaults.exception(false) } catch(e:IllegalStateException) { caught=e.message=="Acme Kotlin fixture failure" }; check(caught); check(AcmeKotlinFaults.exception(true)=="ready"); AcmeKotlinFaults.mainThreadHang(true); AcmeKotlinFaults.backgroundFailure(true); AcmeKotlinFaults.killSelf(true); AcmeKotlinFaults.reset(); println("Acme Kotlin ready") }
`);
    await run(kotlinc, ['-classpath', `${directory}${delimiter}${jar}`, '-d', directory, join(source, 'AcmeKotlinFaults.kt'), kotlinDriver], options);
    // Generate real resource IDs for the activity's semantic controls. This
    // compiles fixture resources, without pretending to package/install the app.
    const aapt2 = resolve(dirname(jar), '../../build-tools/36.0.0/aapt2');
    const manifest = join(directory, 'AndroidManifest.xml');
    await writeFile(manifest, '<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="com.example.acme.diagnostics"><application /></manifest>');
    await run(aapt2, ['compile', '--dir', join(source, '../apps/android/app/src/main/res'), '-o', join(directory, 'resources.zip')], options);
    await run(aapt2, ['link', '-I', jar, '--manifest', manifest, '--java', directory, '-o', join(directory, 'resources.apk'), join(directory, 'resources.zip')], options);
    await run(javac, ['-classpath', `${directory}${delimiter}${jar}`, '-d', directory, join(directory, 'com/example/acme/diagnostics/R.java'), join(source, '../apps/android/app/src/main/java/com/example/acme/diagnostics/MainActivity.java')], options);
    const kotlin = await run(join(dirname(kotlinc), 'kotlin'), ['-classpath', `${directory}${delimiter}${jar}`, 'DriverKt'], options);
    assert.equal(kotlin.stdout.trim(), 'Acme Kotlin ready');
  } finally { await rm(directory, {recursive: true, force: true}); }
});
