import { messages, canadianFrench } from './messages.js';

export const defaultLocale = 'en-US';
export const locales = [
  ['en-CA', 'English (Canada)'], ['en-GB', 'English (UK)'], ['en-US', 'English (US)'],
  ['fr-CA', 'Français (Canada)'], ['fr-FR', 'Français (France)'], ['es', 'Español'],
  ['de', 'Deutsch'], ['nl', 'Nederlands'], ['fi', 'Suomi'], ['sv', 'Svenska'],
  ['nb', 'Norsk bokmål'], ['zh-Hans', '简体中文'], ['ja', '日本語'],
];

const localeBySegment = new Map(locales.map(([locale]) => [locale.toLowerCase(), locale]));
const catalogOrder = ['fr', 'es', 'de', 'nl', 'fi', 'sv', 'nb', 'zh', 'ja'];

function pagePath(pathname, base) {
  const relative = pathname.slice(base.length);
  const slash = relative.indexOf('/');
  return slash >= 0 && localeBySegment.has(relative.slice(0, slash)) ? relative.slice(slash + 1) : relative;
}

export function localizeHref(href, locale, base = '/') {
  if (!href.startsWith(base) || href.startsWith('//')) return href;
  const path = pagePath(href, base);
  if (path && !/^(?:workbench|shield|testrun)(?:\/|$)/.test(path)) return href;
  if (locale === defaultLocale) return `${base}${path}`;
  return `${base}${locale.toLowerCase()}/${path || 'workbench/'}`;
}

// Only the Shield overview currently translates its main content. Other
// locale routes keep translated navigation around an English source page.
export function localeMetadata(url, base = '/') {
  const englishPath = localizeHref(url.pathname, defaultLocale, base);
  const translated = englishPath === `${base}shield/`;
  const { locale } = websiteLocale(url, base);
  return {
    canonicalPath: translated && !locale.startsWith('en-') ? url.pathname : englishPath,
    alternates: translated ? locales.filter(([tag]) => tag === defaultLocale || !tag.startsWith('en-'))
      .map(([tag]) => ({ tag, href: localizeHref(url.pathname, tag, base) })) : [],
  };
}

export function websiteLocale(url, base = '/') {
  const segment = url.pathname.slice(base.length).split('/')[0];
  const locale = localeBySegment.get(segment) ?? defaultLocale;
  const language = locale.split('-')[0];
  const column = catalogOrder.indexOf(language);

  function t(message, values = {}) {
    const regional = locale === 'fr-CA' && Object.hasOwn(canadianFrench, message) ? canadianFrench[message] : undefined;
    const translated = Object.hasOwn(messages, message) ? messages[message][column] : undefined;
    return (regional ?? translated ?? message).replace(/\{(\w+)\}/g, (whole, name) => Object.hasOwn(values, name) ? values[name] : whole);
  }

  return { locale, t, href: (url) => localizeHref(url, locale, base) };
}
