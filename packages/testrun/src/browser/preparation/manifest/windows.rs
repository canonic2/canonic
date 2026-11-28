//! Verified x64 archives for the native Windows browser runtime.

use super::Artifact;

pub(super) const NODE_EXECUTABLE_SHA256: &str =
    "2ffe3acc0458fdde999f50d11809bbe7c9b7ef204dcf17094e325d26ace101d8";

pub(super) static NODE: Artifact = Artifact {
    id: "node-24.12.0-win-x64",
    url: "https://nodejs.org/dist/v24.12.0/node-v24.12.0-win-x64.zip",
    sha256: "9c125f61ae947b52e779095830f9cac267846a043ef7192183c84016aaad2812",
    bytes: 36_361_909,
    directory: "node-24.12.0",
    entry: "node-v24.12.0-win-x64/node.exe",
    zip: true,
    license: "node-v24.12.0-win-x64/LICENSE",
};

pub(super) static FIREFOX: Artifact = Artifact {
    id: "firefox-1555-win64",
    url: "https://cdn.playwright.dev/dbazure/download/playwright/builds/firefox/1555/firefox-win64.zip",
    sha256: "410e14bca6099c089c2092df67803320aab70433a754aae356c4c2e990cf6b77",
    bytes: 129_834_277,
    directory: "browsers/firefox-1555",
    entry: "firefox/firefox.exe",
    zip: true,
    license: "about:license (inside the Firefox distribution); https://www.mozilla.org/MPL/2.0/",
};

pub(super) static WEBKIT: Artifact = Artifact {
    id: "webkit-2370-win64",
    url: "https://cdn.playwright.dev/dbazure/download/playwright/builds/webkit/2370/webkit-win64.zip",
    sha256: "350cbf3b526414a83cd09c2481a4d134e67f4d52850093d703545dee6fdcb047",
    bytes: 63_138_602,
    directory: "browsers/webkit-2370",
    entry: "Playwright.exe",
    zip: true,
    license: "https://webkit.org/licensing-webkit/",
};

pub(super) static WINLDD: Artifact = Artifact {
    id: "winldd-1007-win64",
    url: "https://cdn.playwright.dev/dbazure/download/playwright/builds/winldd/1007/winldd-win64.zip",
    sha256: "0069f0d11d4ad6df068a068c003d22fe7dbec192a47bba64b2e115e9c8ce41d8",
    bytes: 128_684,
    directory: "browsers/winldd-1007",
    entry: "PrintDeps.exe",
    zip: true,
    license: "https://github.com/microsoft/playwright/blob/v1.64.1/browser_patches/winldd/PrintDeps.cpp",
};

pub(super) static CHROME: Artifact = Artifact {
    id: "chrome-156.0.8078.47-win64",
    url: "https://storage.googleapis.com/chrome-for-testing-public/156.0.8078.47/win64/chrome-win64.zip",
    sha256: "a4e0d57245e167e98a9d465af1e3774d2b4ec9f04e483d19bcd97a6f63cf46dd",
    bytes: 209_154_801,
    directory: "browsers/chrome-156.0.8078.47",
    entry: "chrome-win64/chrome.exe",
    zip: true,
    license: "https://www.google.com/intl/en/chrome/terms/; bundled chrome-win64/ABOUT",
};
