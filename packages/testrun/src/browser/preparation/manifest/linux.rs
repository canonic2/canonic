//! Pinned artifacts for Ubuntu 26.04 x86_64.

use super::Artifact;

pub(super) const NODE_EXECUTABLE_SHA256: &str =
    "16143bdaa79716e871d3d9b2f50ce680bca293eba7f0c3fc1d004ed2258fc839";

pub(super) static NODE: Artifact = Artifact {
    id: "node-24.12.0-linux-x64",
    url: "https://nodejs.org/dist/v24.12.0/node-v24.12.0-linux-x64.tar.gz",
    sha256: "6159227e0af7d7c3c6bb2fa900452b04a6cb8841a702a79acc613209d70b04d0",
    bytes: 56_331_493,
    directory: "node-24.12.0",
    entry: "node-v24.12.0-linux-x64/bin/node",
    zip: false,
    license: "node-v24.12.0-linux-x64/LICENSE",
};

pub(super) static FIREFOX: Artifact = Artifact {
    id: "firefox-1555-ubuntu24-x64",
    url: "https://cdn.playwright.dev/dbazure/download/playwright/builds/firefox/1555/firefox-ubuntu-24.04.zip",
    sha256: "564f74fa3c46cb83c6252145cf1569c95236c8c404d6a5a80cc736cf32265f0c",
    bytes: 116_377_639,
    directory: "browsers/firefox-1555",
    entry: "firefox/firefox",
    zip: true,
    license: "about:license (inside the Firefox distribution); https://www.mozilla.org/MPL/2.0/",
};

pub(super) static WEBKIT: Artifact = Artifact {
    id: "webkit-2370-ubuntu26-x64",
    url: "https://cdn.playwright.dev/dbazure/download/playwright/builds/webkit/2370/webkit-ubuntu-26.04.zip",
    sha256: "d0667d52a00058b7fd8b5e338b54520bbb2c835c999f468790bec47df4534f01",
    bytes: 100_816_904,
    directory: "browsers/webkit-2370",
    entry: "pw_run.sh",
    zip: true,
    license: "https://webkit.org/licensing-webkit/",
};

pub(super) static CHROME: Artifact = Artifact {
    id: "chrome-156.0.8078.47-linux-x64",
    url: "https://storage.googleapis.com/chrome-for-testing-public/156.0.8078.47/linux64/chrome-linux64.zip",
    sha256: "e6c94490f0b6b337225aeb2d2a5034858516b7017356980ba94f5176d8774a50",
    bytes: 200_206_486,
    directory: "browsers/chrome-156.0.8078.47",
    entry: "chrome-linux64/chrome",
    zip: true,
    license: "https://www.google.com/intl/en/chrome/terms/; bundled chrome-linux64/ABOUT",
};
