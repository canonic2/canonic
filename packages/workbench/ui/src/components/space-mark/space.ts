/** Display data supplied by a host. No project or host operations live here. */
export interface Space {
  id?: string;
  name?: string;
  initial?: string;
  color?: string;
  icon?: string;
  image?: string;
  root?: string;
  removable?: boolean;
}

export type IconRenderer = (name: string, size?: number) => Element;

const COLORS: Record<string, string> = {
  blue: '#2f5cff', green: '#2f7d55', orange: '#b4572a', purple: '#7246d6',
  pink: '#b83a78', teal: '#17788a', red: '#c2362f', yellow: '#d9a400', gray: '#5f6368',
};

export function markColor(value = ''): string {
  return /^#(?:[0-9a-f]{3}|[0-9a-f]{6})$/i.test(value) ? value : COLORS[value] || '';
}

export function isLight(hex: string): boolean {
  let digits = hex.slice(1);
  if (digits.length === 3) digits = digits.replace(/./g, '$&$&');
  const channels = [0, 2, 4].map(at => {
    const value = parseInt(digits.slice(at, at + 2), 16) / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return .2126 * channels[0]! + .7152 * channels[1]! + .0722 * channels[2]! > .4;
}
