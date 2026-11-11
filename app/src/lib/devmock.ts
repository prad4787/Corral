// Development-only design preview: when the UI runs in a normal browser
// (`pnpm dev`, no Tauri), answer backend calls with realistic sample data so
// every screen can be reviewed and screenshotted. Never active in the app.

const sites = [
  {
    name: "ecommerce", host: "ecommerce.test", path: "/Users/dev/Code/ecommerce", origin: "parked",
    driver: { kind: "laravel", docroot: "/Users/dev/Code/ecommerce/public", front_controller: "index.php" },
    php: "8.4", isolated: false, secure: true, processes: ["queue"], url: "https://ecommerce.test", driver_label: "Laravel",
  },
  {
    name: "el-billing", host: "el-billing.test", path: "/Users/dev/Code/el-billing", origin: "parked",
    driver: { kind: "laravel", docroot: "/Users/dev/Code/el-billing/public", front_controller: "index.php" },
    php: "8.3", isolated: true, secure: true, processes: [], url: "https://el-billing.test", driver_label: "Laravel",
  },
  {
    name: "docs", host: "docs.test", path: "/Users/dev/Sites/docs", origin: "linked",
    driver: { kind: "static", docroot: "/Users/dev/Sites/docs", front_controller: null },
    php: "8.4", isolated: false, secure: false, processes: [], url: "http://docs.test", driver_label: "Static",
  },
];

const setupPreview = new URLSearchParams(location.search).has("setup");

const overview = () => setupPreview ? {
  ...fullOverview(), caddy_installed: true, php: [], parked: [], sites: [], path_configured: false,
  system: { dns: false, dns_foreign: false, https: false, ports: false, ports_busy: false },
} : fullOverview();

const fullOverview = () => ({
  sites,
  node: [{ major: 24, version: "v24.11.0", is_default: true }],
  php: [{ version: "8.4", is_default: true, build: "standard" }, { version: "8.3", is_default: false, build: "extended" }],
  services: [
    { name: "mysql", kind: "mysql", label: "MySQL", version: "8.4.11", port: 3306, enabled: true, listening: true },
    { name: "valkey", kind: "valkey", label: "Valkey (Redis-compatible)", version: "9.1.3", port: 6379, enabled: false, listening: false },
  ],
  tld: "test",
  parked: ["/Users/dev/Code"],
  default_php: "8.4",
  running: true,
  running_elsewhere: false,
  caddy_installed: true,
  composer_installed: true,
  path_configured: true,
  autostart: false,
  http_port: 80,
  https_port: 443,
  system: { dns: true, dns_foreign: false, https: true, ports: true, ports_busy: false },
  adminer: true,
  mail_port: 2525,
  core: [
    { id: "web", name: "Web server (Caddy)", detail: "HTTPS 443 → 18443 · HTTP 80 → 18080", running: true },
    { id: "dns", name: "DNS", detail: "*.test → 127.0.0.1 · UDP 15353", running: true },
    { id: "php", name: "PHP-FPM 8.4", detail: "php-fpm-8.4.sock", running: true },
    { id: "php", name: "PHP-FPM 8.3", detail: "php-fpm-8.3.sock", running: true },
    { id: "mail", name: "Mail catcher", detail: "SMTP 127.0.0.1:2525", running: true },
    { id: "dumps", name: "Dump server", detail: "dump() and watcher events", running: true },
  ],
  tools: [
    { id: "adminer", label: "Adminer", description: "One page for MySQL, PostgreSQL and SQLite.", host: "adminer.test", installed: true, running: true, note: null },
    { id: "phpmyadmin", label: "phpMyAdmin", description: "Full MySQL administration.", host: "phpmyadmin.test", installed: false, running: false, note: "Needs PHP 8.3 or older with the mysqli extension (Corral’s own PHP builds include it)." },
    { id: "pgweb", label: "pgweb", description: "PostgreSQL browser and SQL editor.", host: "pgweb.test", installed: false, running: false, note: "Add PostgreSQL to connect automatically." },
    { id: "redis-commander", label: "Redis Commander", description: "Browse Redis/Valkey keys and run commands in its console.", host: "redis.test", installed: true, running: true, note: null },
  ],
  catalog: [
    { kind: "mongodb", label: "MongoDB", version: "8.2.12", license: "SSPL", available: true, installed: false },
    { kind: "postgres", label: "PostgreSQL", version: "18.6", license: "PostgreSQL License", available: true, installed: false },
    { kind: "valkey", label: "Valkey (Redis-compatible)", version: "9.1.3", license: "BSD-3-Clause", available: true, installed: true },
    { kind: "mysql", label: "MySQL", version: "8.4.11", license: "GPL-2.0", available: true, installed: true },
  ],
  shares: [] as [string, string][],
});

const now = Date.now() / 1000;
const mails = [
  { id: "a1", received: now - 120, from: "Shop <orders@ecommerce.test>", to: ["ann@example.com"], subject: "Your order #1042 has shipped", size: 18213, attachments: 1, envelope_from: "orders@ecommerce.test", envelope_to: ["ann@example.com"] },
  { id: "a2", received: now - 3600, from: "Billing <billing@el-billing.test>", to: ["finance@example.com"], subject: "Invoice INV-2026-0311", size: 9120, attachments: 0, envelope_from: "billing@el-billing.test", envelope_to: ["finance@example.com"] },
  { id: "a3", received: now - 90000, from: "Laravel <hello@example.com>", to: ["dev@example.com"], subject: "Reset Password Notification", size: 6402, attachments: 0, envelope_from: "hello@example.com", envelope_to: ["dev@example.com"] },
];

const handlers: Record<string, (args: any) => unknown> = {
  overview,
  check_update: () => ({ current: "0.4.0", latest: "v0.4.0", newer: false, url: null }),
  project_info: ({ site }) => ({
    laravel: site !== "docs",
    watchers: site === "ecommerce",
    db: site === "docs" ? null : { connection: "mysql", database: site.replace("-", ""), username: site === "el-billing" ? "root" : "app", has_password: true, service: "mysql", needs_dedicated_user: site === "el-billing" },
    processes: site === "ecommerce" ? ["queue"] : [],
    reverb_available: site === "ecommerce",
  }),
  mail_list: () => mails,
  mail_detail: ({ id }) => {
    const m = mails.find((x) => x.id === id)!;
    return { ...m, cc: [], date: null, html: `<div style="font:15px -apple-system;padding:24px;color:#222"><h2 style="margin:0 0 12px">Thanks for your order</h2><p>Order #1042 is on its way. Track it from your account.</p></div>`, text: "Order #1042 is on its way.", headers: [["From", m.from], ["To", m.to[0]], ["Subject", m.subject], ["MIME-Version", "1.0"]], attachment_list: m.attachments ? [{ index: 0, name: "invoice-1042.pdf", content_type: "application/pdf", size: 48211 }] : [] };
  },
  dumps_list: () => [
    { id: 2, time: now - 30, label: null, html: "", text: 'array:3 [\n  "order" => 1042\n  "items" => array:2 [\n    0 => "tea"\n    1 => "cake"\n  ]\n  "paid" => true\n]', file: "/Users/dev/Code/ecommerce/app/Http/Controllers/OrderController.php", line: 48, sapi: "fpm-fcgi", uri: "/orders/1042" },
    { id: 1, time: now - 400, label: null, html: "", text: 'App\\Models\\User {#1290\n  +id: 7\n  +email: "ann@example.com"\n}', file: "/Users/dev/Code/ecommerce/routes/web.php", line: 22, sapi: "fpm-fcgi", uri: "/" },
  ],
  events_list: () => [
    { id: 6, kind: "query", time: now - 20, request_id: "x", request: {}, data: { sql: 'select * from "orders" where "id" = ? limit 1', time_ms: 0.82 } },
    { id: 5, kind: "http", time: now - 21, request_id: "x", request: {}, data: { method: "POST", url: "https://api.stripe.test/v1/charges", status: 200 } },
    { id: 4, kind: "cache", time: now - 22, request_id: "x", request: {}, data: { op: "miss", key: "settings" } },
    { id: 3, kind: "log", time: now - 23, request_id: "x", request: {}, data: { level: "info", message: "Order 1042 shipped" } },
    { id: 2, kind: "view", time: now - 24, request_id: "x", request: {}, data: { name: "orders.show" } },
    { id: 1, kind: "query", time: now - 25, request_id: "x", request: {}, data: { sql: 'select count(*) as aggregate from "users"', time_ms: 3.4 } },
  ],
  site_logs: () => [
    { time: "2026-11-11 09:41:02", env: "local", level: "error", message: "SQLSTATE[HY000] [2002] Connection refused (Connection: mysql)", details: "#0 vendor/laravel/framework/src/Illuminate/Database/Connectors/Connector.php(66)\n#1 {main}" },
    { time: "2026-11-11 09:40:12", env: "local", level: "info", message: "Order 1042 shipped", details: "" },
  ],
  read_env: () => "APP_NAME=Shop\nAPP_ENV=local\nAPP_URL=https://ecommerce.test\nDB_CONNECTION=mysql\nDB_DATABASE=ecommerce\n",
  php_details: ({ version }) => ({
    version,
    build: version === "8.3" ? "extended" : "standard",
    xdebug: version === "8.4",
    modules: ["bcmath", "bz2", "calendar", "core", "ctype", "curl", "date", "dom", "exif", "fileinfo", "filter", "ftp", "gd", "gmp", "hash", "iconv", "json", "libxml", "mbstring", "mysqlnd", "openssl", "pcntl", "pcre", "pdo", "pdo_mysql", "pdo_pgsql", "pdo_sqlite", "pgsql", "phar", "posix", "random", "redis", "reflection", "session", "simplexml", "soap", "sockets", "spl", "sqlite3", "standard", "tokenizer", "xml", "xmlreader", "xmlwriter", "zip", "zlib", ...(version === "8.3" ? ["mysqli", "intl", "imagick", "apcu", "swoole"] : [])].sort(),
    settings: { memory_limit: "512M", upload_max_filesize: "512M", post_max_size: "256M", max_execution_time: "120", display_errors: "On", error_reporting: "E_ALL & ~E_DEPRECATED", log_errors: "On", "date.timezone": "UTC", "opcache.enable": "1", "opcache.validate_timestamps": "1" },
    overrides: { post_max_size: "256M" },
  }),
  set_php_settings: () => null,
  read_php_ini: () => "; Your PHP overrides\nmemory_limit = 1G\n",
};

export function installMock() {
  const w = window as any;
  if (w.__TAURI_INTERNALS__) return;
  // ?scheme=light|dark forces a scheme; the page background stands in for
  // the macOS sidebar material a browser cannot draw.
  const scheme = new URLSearchParams(location.search).get("scheme");
  if (scheme) document.documentElement.dataset.scheme = scheme;
  const dark = scheme ? scheme === "dark" : matchMedia("(prefers-color-scheme: dark)").matches;
  document.documentElement.style.background = dark ? "#2b2a2c" : "#e4e3e6";
  let cb = 0;
  w.__TAURI_INTERNALS__ = {
    transformCallback: () => ++cb,
    invoke: async (cmd: string, args: any) => {
      if (cmd.startsWith("plugin:")) return cmd.includes("listen") ? ++cb : null;
      const h = handlers[cmd];
      await new Promise((r) => setTimeout(r, 120));
      return h ? h(args ?? {}) : overview();
    },
  };
  w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
}
