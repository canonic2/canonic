import type { ProcessRecord } from '../../platform/processes.ts';

export interface RuntimeTask {
  checkout: string;
  ports: Record<string, number>;
  port: number;
  url: string;
  env: NodeJS.ProcessEnv;
  composeName: string;
  composeOutput: string;
  composeStarted: boolean;
  composeFile?: string;
  process: ProcessRecord | null;
}

export type RuntimeFailure = Error & { cleanupTask?: RuntimeTask };

export interface ComposeModel {
  volumes?: Record<string, { external?: boolean; name?: string }>;
  networks?: Record<string, { external?: boolean; name?: string }>;
  services?: Record<
    string,
    {
      container_name?: string;
      network_mode?: string;
      privileged?: boolean;
      ports?: { host_ip?: string; published?: string | number }[];
      volumes?: { type?: string; source: string }[];
    }
  >;
}
