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
  node: { major: number; version: string; is_default: boolean }[];
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
  system: SystemStatus;
  adminer: boolean;
  shares: [string, string][];
}

export interface SystemStatus {
  dns: boolean;
  dns_foreign: boolean;
  https: boolean;
  ports: boolean;
  ports_busy: boolean;
}

export interface EnvDb {
  connection: string;
  database: string | null;
  username: string | null;
  has_password: boolean;
  service: string | null;
  needs_dedicated_user: boolean;
}

export interface ProjectInfo {
  laravel: boolean;
  watchers: boolean;
  db: EnvDb | null;
  processes: string[];
  reverb_available: boolean;
}

export interface MailSummary {
  id: string;
  received: number;
  from: string;
  to: string[];
  subject: string;
  size: number;
  attachments: number;
  envelope_from: string;
  envelope_to: string[];
}

export interface MailDetail extends MailSummary {
  cc: string[];
  date: string | null;
  html: string | null;
  text: string | null;
  headers: [string, string][];
  attachment_list: { index: number; name: string; content_type: string; size: number }[];
}

export interface Dump {
  id: number;
  time: number;
  label: string | null;
  html: string;
  text: string;
  file: string | null;
  line: number | null;
  sapi: string | null;
  uri: string | null;
}

export interface WatchEvent {
  id: number;
  kind: "query" | "http" | "log" | "view" | "cache" | "job";
  time: number;
  request_id: string | null;
  request: { method?: string | null; host?: string | null; uri?: string | null; command?: string | null };
  data: Record<string, any>;
}

export interface LogEntry {
  time: string;
  env: string;
  level: string;
  message: string;
  details: string;
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
  install: (what: "web" | "php" | "mysql" | "valkey" | "postgres" | "mongodb" | "adminer", version?: string) =>
    invoke<Overview>("install_component", { what, version }),
  setServiceEnabled: (name: string, enabled: boolean) =>
    invoke<Overview>("set_service_enabled", { name, enabled }),
  setShellPath: (enabled: boolean) => invoke<Overview>("set_shell_path", { enabled }),
  setAutostart: (enabled: boolean) => invoke<Overview>("set_autostart", { enabled }),
  openIn: (path: string, target: "editor" | "terminal" | "tinker") =>
    invoke<void>("open_in", { path, target }),
  readEnv: (site: string) => invoke<string | null>("read_env", { site }),
  writeEnv: (site: string, contents: string) => invoke<void>("write_env", { site, contents }),
  readPhpIni: (version: string) => invoke<string>("read_php_ini", { version }),
  writePhpIni: (version: string, contents: string) =>
    invoke<void>("write_php_ini", { version, contents }),
  nodeInstall: (major?: number) => invoke<Overview>("node_install", { major }),
  nodeUse: (major: number) => invoke<Overview>("node_use", { major }),
  newProject: (name: string, kit: string) => invoke<Overview>("new_project", { name, kit }),
  mailList: () => invoke<MailSummary[]>("mail_list", {}),
  mailDetail: (id: string) => invoke<MailDetail>("mail_detail", { id }),
  mailDelete: (id: string) => invoke<void>("mail_delete", { id }),
  mailClear: () => invoke<number>("mail_clear"),
  mailSaveAttachment: (id: string, index: number) =>
    invoke<string>("mail_save_attachment", { id, index }),
  dumpsList: () => invoke<Dump[]>("dumps_list", {}),
  dumpsClear: () => invoke<void>("dumps_clear"),
  systemSetup: () => invoke<Overview>("system_setup"),
  systemUndo: () => invoke<Overview>("system_undo"),
  projectInfo: (site: string) => invoke<ProjectInfo>("project_info", { site }),
  setWatchers: (site: string, enable: boolean) => invoke<void>("set_watchers", { site, enable }),
  createDatabase: (site: string, dedicatedUser?: string) =>
    invoke<string>("create_database", { site, dedicatedUser }),
  setSiteProcess: (site: string, kind: string, on: boolean) =>
    invoke<void>("set_site_process", { site, kind, on }),
  adminerRemove: () => invoke<Overview>("adminer_remove"),
  shareStart: (site: string) => invoke<Overview>("share_start", { site }),
  shareStop: (site: string) => invoke<Overview>("share_stop", { site }),
  uninstall: (deleteData: boolean) => invoke<void>("uninstall_corral", { deleteData }),
  eventsList: () => invoke<WatchEvent[]>("events_list", {}),
  eventsClear: () => invoke<void>("events_clear"),
  siteLogs: (site: string, level?: string) => invoke<LogEntry[]>("site_logs", { site, level }),
};
