// Build only a prepared owned Flutter fixture: an Android Debug APK, or with --ios an
// iOS simulator Debug app. No emulator, simulator or device.
import assert from 'node:assert/strict';
import {spawn} from 'node:child_process';
import {isAbsolute, join} from 'node:path';
import {installFlutterSources, ownedFlutterApp} from './diagnostic-flutter-app.mjs';

const ios = process.argv[2] === '--ios';
const [root, flutter, sdk, javaHome] = process.argv.slice(ios ? 3 : 2);
for (const path of ios ? [root, flutter] : [root, flutter, sdk, javaHome]) assert.ok(path && isAbsolute(path), ios ? 'Select owned fixture root and Flutter launcher' : 'Select owned fixture root, Flutter launcher, Android SDK and JDK');
if (ios) assert.equal(process.platform, 'darwin', 'iOS simulator builds require macOS');
const {project} = await ownedFlutterApp(root);
await installFlutterSources(project);
const env = ios ? {...process.env} : {...process.env, ANDROID_HOME: sdk, ANDROID_SDK_ROOT: sdk, JAVA_HOME: javaHome};
async function run(args) {
  await new Promise((accept, reject) => {
    const child = spawn(flutter, args, {cwd: project, env, stdio: 'inherit', timeout: 3600000});
    child.once('error', reject);
    child.once('exit', (code, signal) => code === 0 ? accept() : reject(Error(`Fixture build failed: flutter ${args[0]} (${code ?? signal})`)));
  });
}
await run(['pub', 'get']);
await run(['test']);
if (ios) {
  // Without `flutter run`, the engine reads Dart profiling from the app's Info.plist.
  await new Promise((accept, reject) => {
    const child = spawn('/usr/bin/plutil', ['-replace', 'FLTEnableDartProfiling', '-bool', 'YES', join(project, 'ios/Runner/Info.plist')], {stdio: 'inherit'});
    child.once('error', reject);
    child.once('exit', code => code === 0 ? accept() : reject(Error('Cannot enable fixture Dart profiling')));
  });
  await run(['build', 'ios', '--simulator', '--debug']);
  console.log(JSON.stringify({framework: 'flutter', app: join(project, 'build/ios/iphonesimulator/Runner.app')}));
} else {
  await run(['build', 'apk', '--debug', '--target-platform', 'android-x64']);
  console.log(JSON.stringify({framework: 'flutter', apk: join(project, 'build/app/outputs/flutter-apk/app-debug.apk')}));
}
