// Owned Flutter fixture identity and source installation.
import assert from 'node:assert/strict';
import {access, cp, readFile} from 'node:fs/promises';
import {isAbsolute, join} from 'node:path';

export const flutterAppId = 'com.example.acme.acme_diagnostics';
// `flutter create` derives the iOS bundle identifier in lower camel case.
export const flutterIosAppId = 'com.example.acme.acmeDiagnostics';
const fixtures = new URL('../tests/fixtures/diagnostics/', import.meta.url);
export const activityPath = project => join(project, 'android/app/src/main/kotlin', ...flutterAppId.split('.'), 'MainActivity.kt');
export const appDelegatePath = project => join(project, 'ios/Runner/AppDelegate.swift');

/** Verify the owner marker and the generated Android and iOS application identities. */
export async function ownedFlutterApp(appRoot) {
  assert.ok(appRoot && isAbsolute(appRoot), 'Select an absolute owned Flutter fixture root');
  const owner = JSON.parse(await readFile(join(appRoot, 'acme-fixture-owner.json'), 'utf8'));
  assert.equal(owner.framework, 'flutter', 'Owned fixture must be a Flutter project');
  const project = join(appRoot, 'project');
  const gradle = await readFile(join(project, 'android/app/build.gradle.kts'), 'utf8');
  for (const key of ['namespace', 'applicationId'])
    assert.match(gradle, new RegExp(`^\\s*${key}\\s*=\\s*"${flutterAppId.replaceAll('.', '\\.')}"\\s*$`, 'm'), `Generated Android ${key} must be ${flutterAppId}`);
  const xcode = await readFile(join(project, 'ios/Runner.xcodeproj/project.pbxproj'), 'utf8');
  const bundles = new Set([...xcode.matchAll(/PRODUCT_BUNDLE_IDENTIFIER = ([^;]+);/g)].map(match => match[1]).filter(id => !id.endsWith('.RunnerTests')));
  assert.deepEqual([...bundles], [flutterIosAppId], `Generated iOS bundle identifier must be ${flutterIosAppId}`);
  return {owner, project};
}

/** Refresh the checked-in Dart sources, widget test and native breadcrumb handlers. */
export async function installFlutterSources(project) {
  // The generator's activity and app delegate must exist before they are replaced.
  for (const path of [activityPath(project), appDelegatePath(project)]) await access(path);
  for (const name of ['main.dart', 'acme_faults.dart']) await cp(new URL(`flutter/${name}`, fixtures), join(project, 'lib', name));
  await cp(new URL('apps/flutter/diagnostics_test.dart', fixtures), join(project, 'test/widget_test.dart'));
  await cp(new URL('apps/flutter/MainActivity.kt', fixtures), activityPath(project));
  await cp(new URL('apps/flutter/AppDelegate.swift', fixtures), appDelegatePath(project));
}
