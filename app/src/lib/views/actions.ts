// Actions shared by several views (folder pickers, new project).
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { api } from "$lib/api";
import { app } from "$lib/app.svelte";

export async function pickFolder(title: string): Promise<string | null> {
  const dir = await openDialog({ directory: true, multiple: false, title });
  return typeof dir === "string" ? dir : null;
}

export async function addParkedFolder() {
  const dir = await pickFolder("Choose a folder whose projects Corral should serve");
  if (dir) await app.run("park", () => api.park(dir));
}

export async function linkProject() {
  const dir = await pickFolder("Choose a project folder");
  if (dir) await app.run("link", () => api.link(dir));
}
