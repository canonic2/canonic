export interface RuntimeRecipe {
  command: string[];
  provision?: string[];
  env?: Record<string, string>;
  ports: string[];
  previewPort: string;
  readyPath: string;
  timeoutMs: number;
  provisionTimeoutMs: number;
  compose?: { file: string; waitTimeoutSeconds: number };
}

export interface ProjectConfig {
  version: 1;
  $schema?: string;
  ide?: { settings?: Record<string, unknown> };
  runtime?: RuntimeRecipe;
}
