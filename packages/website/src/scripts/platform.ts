/* The download to recommend for the visitor's computer. Macs get the Apple
   silicon build; the browser can't tell Intel apart reliably, so `alt` names
   the other build of the same system. Null when the system is unknown. */
export interface Recommendation { target: string; alt: string; ask: string; name: string }

const picks: Record<string, Recommendation> = {
  mac: { target: 'darwin-arm64', alt: 'darwin-x64', ask: 'On a Mac with Intel?', name: 'Mac' },
  win: { target: 'win32-x64', alt: 'win32-arm64', ask: 'On Windows on Arm?', name: 'Windows' },
  linux: { target: 'linux-x64', alt: 'linux-arm64', ask: 'On Linux on Arm?', name: 'Linux' },
};

export function recommendation(): Recommendation | null {
  let platform = '';
  try {
    const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
    platform = (nav.userAgentData?.platform || nav.platform || nav.userAgent || '').toLowerCase();
  } catch {
    return null;
  }
  const os = platform.includes('mac') ? 'mac' : platform.includes('win') ? 'win'
    : platform.includes('linux') || platform.includes('x11') ? 'linux' : null;
  return os ? picks[os] : null;
}

export const fileName = (target: string) => `canonic-workbench-${target}.vsix`;
