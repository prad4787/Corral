// Shared UI state and actions. Views read `app.*` and call `app.run`/`app.act`
// so busy/error handling is consistent everywhere.
import { api, type Overview } from "$lib/api";

export type View = "sites" | "site" | "services" | "runtimes" | "mail" | "dumps" | "settings" | "php";

class AppState {
  data = $state<Overview | null>(null);
  view = $state<View>("sites");
  selected = $state<string | null>(null);
  /** PHP version shown by the "php" view. */
  phpVersion = $state<string | null>(null);
  query = $state("");
  /** Label of the action in progress (disables controls, shows progress). */
  busy = $state<string | null>(null);
  error = $state<string | null>(null);
  notice = $state<string | null>(null);
  skipSetup = $state(false);

  site = $derived(this.data?.sites.find((s) => s.name === this.selected) ?? null);

  needsSetup = $derived(
    !!this.data &&
      !this.skipSetup &&
      (!this.data.caddy_installed ||
        this.data.php.length === 0 ||
        this.data.parked.length + this.data.sites.length === 0 ||
        !(this.data.system.dns && this.data.system.https && this.data.system.ports)),
  );

  status = $derived(
    !this.data
      ? "Loading…"
      : this.data.running
        ? "Running"
        : this.data.running_elsewhere
          ? "Running from Terminal"
          : "Stopped",
  );

  /** Run an action that returns a fresh overview. */
  async run(label: string, op: () => Promise<Overview>) {
    this.busy = label;
    this.error = null;
    try {
      this.data = await op();
      if (this.selected && !this.data.sites.some((s) => s.name === this.selected)) {
        this.selected = null;
        if (this.view === "site") this.view = "sites";
      }
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    } finally {
      this.busy = null;
    }
  }

  /** Run any other action. */
  async act(label: string, op: () => Promise<void>) {
    this.busy = label;
    this.error = null;
    try {
      await op();
      return true;
    } catch (e) {
      this.error = String(e);
      return false;
    } finally {
      this.busy = null;
    }
  }

  refresh() {
    if (this.busy) return;
    api.overview().then((d) => (this.data = d)).catch(() => {});
  }

  openSite(name: string) {
    this.selected = name;
    this.view = "site";
  }

  /** Public HTTPS URL for a host, honouring a non-default port. */
  url(host: string) {
    const port = this.data?.https_port ?? 443;
    return `https://${host}${port === 443 ? "" : ":" + port}`;
  }
}

export const app = new AppState();

export function fmtTime(secs: number) {
  const d = new Date(secs * 1000);
  const today = new Date();
  return d.toDateString() === today.toDateString()
    ? d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" })
    : d.toLocaleDateString([], { month: "short", day: "numeric" });
}

export function fmtSize(n: number) {
  return n < 1024 ? `${n} bytes` : n < 1048576 ? `${(n / 1024).toFixed(0)} KB` : `${(n / 1048576).toFixed(1)} MB`;
}
