import { invoke } from "@tauri-apps/api/core";

export type DriverKind =
  | "laravel" | "statamic" | "symfony" | "craft" | "word-press" | "drupal"
  | "generic-public" | "basic-php" | "static";

export interface Site {
  name: string;
  host: string;
  path: string;
  origin: "parked" | "linked";
  driver: { kind: DriverKind; docroot: string; front_controller: string | null };
  php: string | null;
  isolated: boolean;
  secure: boolean;
  url: string;
  driver_label: string;
}

export interface Service {
  name: string;
  kind: string;
  label: string;
  version: string;
  port: number;
  enabled: boolean;
  listening: boolean;
}

export interface Overview {
  sites: Site[];
  php: { version: string; is_default: boolean }[];
  services: Service[];
  tld: string;
  parked: string[];
  default_php: string | null;
  running: boolean;
  running_elsewhere: boolean;
  caddy_installed: boolean;
  composer_installed: boolean;
  path_configured: boolean;
  autostart: boolean;
  http_port: number;
  https_port: number;
}

export const api = {
  overview: () => invoke<Overview>("overview"),
  park: (dir: string) => invoke<Overview>("park", { dir }),
  unpark: (dir: string) => invoke<Overview>("unpark", { dir }),
  link: (path: string, name?: string) => invoke<Overview>("link", { path, name }),
  unlink: (name: string) => invoke<Overview>("unlink", { name }),
  setSitePhp: (name: string, version: string | null) =>
    invoke<Overview>("set_site_php", { name, version }),
  setSiteSecure: (name: string, secure: boolean) =>
    invoke<Overview>("set_site_secure", { name, secure }),
  setDefaultPhp: (version: string) => invoke<Overview>("set_default_php", { version }),
  start: () => invoke<Overview>("start_engine"),
  stop: () => invoke<Overview>("stop_engine"),
  install: (what: "web" | "php" | "mysql", version?: string) =>
    invoke<Overview>("install_component", { what, version }),
  setServiceEnabled: (name: string, enabled: boolean) =>
    invoke<Overview>("set_service_enabled", { name, enabled }),
  setShellPath: (enabled: boolean) => invoke<Overview>("set_shell_path", { enabled }),
  setAutostart: (enabled: boolean) => invoke<Overview>("set_autostart", { enabled }),
  openIn: (path: string, target: "editor" | "terminal") => invoke<void>("open_in", { path, target }),
};
