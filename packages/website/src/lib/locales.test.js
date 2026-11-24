import assert from 'node:assert/strict';
import { test } from 'node:test';
import { defaultLocale, locales, localizeHref, localeMetadata, websiteLocale } from './locales.js';
import { messages } from './messages.js';

test('switching locales keeps the current guide, deployment base and fragment', () => {
  const path = '/canonic/fr-ca/shield/docs/getting-started/?q=privacy#install';
  assert.equal(localizeHref(path, 'ja', '/canonic/'), '/canonic/ja/shield/docs/getting-started/?q=privacy#install');
  assert.equal(localizeHref(path, defaultLocale, '/canonic/'), '/canonic/shield/docs/getting-started/?q=privacy#install');
  assert.equal(localizeHref('/canonic/', 'nb', '/canonic/'), '/canonic/nb/workbench/');
  assert.equal(localizeHref('/canonic/images/overview.jpg', 'ja', '/canonic/'), '/canonic/images/overview.jpg');
  assert.equal(localizeHref('//example.com/shield/', 'ja'), '//example.com/shield/');
});

test('site translations use the URL locale and keep product names in substitutions', () => {
  const french = websiteLocale(new URL('https://example.com/fr-ca/shield/'));
  const japanese = websiteLocale(new URL('https://example.com/ja/shield/'));
  assert.equal(french.locale, 'fr-CA');
  assert.equal(french.t('Build from source'), 'Compiler à partir du code source');
  assert.equal(japanese.t('Get {product}', { product: 'Shield' }), 'Shield を入手');
  assert.equal(japanese.t('Untranslated source'), 'Untranslated source');
  assert.equal(websiteLocale(new URL('https://example.com/workbench/')).locale, defaultLocale);
});

test('each translated site message covers the requested non-English languages', () => {
  for (const [message, row] of Object.entries(messages)) {
    assert.equal(row.length, new Set(locales.map(([tag]) => tag.split('-')[0])).size - 1, message);
    const placeholders = (value) => [...value.matchAll(/\{\w+\}/g)].map(([name]) => name).sort();
    for (const translation of row) assert.deepEqual(placeholders(translation), placeholders(message), message);
  }
});

test('English-body locale routes share the source canonical without translation alternates', () => {
  for (const route of ['workbench/', 'workbench/install/', 'workbench/changelog/', 'workbench/docs/', 'shield/docs/getting-started/']) {
    const metadata = localeMetadata(new URL(`https://example.com/canonic/ja/${route}`), '/canonic/');
    assert.equal(metadata.canonicalPath, `/canonic/${route}`);
    assert.deepEqual(metadata.alternates, []);
  }
});

test('translated Shield overviews advertise only canonical language variants', () => {
  for (const prefix of ['', 'ja/', 'en-ca/', 'en-gb/']) {
    const metadata = localeMetadata(new URL(`https://example.com/${prefix}shield/`));
    assert.equal(metadata.canonicalPath, prefix === 'ja/' ? '/ja/shield/' : '/shield/');
    assert.ok(metadata.alternates.some(option => option.tag === 'ja' && option.href === '/ja/shield/'));
    assert.ok(metadata.alternates.some(option => option.tag === 'en-US' && option.href === '/shield/'));
    assert.ok(!metadata.alternates.some(option => ['en-CA', 'en-GB'].includes(option.tag)));
  }
});

test('localized search feedback interpolates counts and translates empty and failure states', () => {
  const { t } = websiteLocale(new URL('https://example.com/ja/shield/docs/'));
  assert.equal(t('{count} matching guides', { count: 3 }), '3 件のガイドが見つかりました');
  assert.equal(t('No matching guides'), '該当するガイドはありません');
  assert.equal(t('Search unavailable. Browse the guides below.'), '検索を利用できません。下のガイドをご覧ください。');
  assert.equal(t('Copy code'), 'コードをコピー');
});
