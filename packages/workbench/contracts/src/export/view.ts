// A browser view requested for export.
export interface CurrentView {
  page: string; state?: string | null; lens?: string | null;
  size?: string; width: number; height: number;
}
