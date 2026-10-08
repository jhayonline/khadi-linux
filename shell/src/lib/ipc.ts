// The Rust boundary.
//
// One call per panel, not one per number: a chatty IPC boundary is the usual
// way a webview UI ends up janky, and every panel refreshes on a timer anyway.

import { invoke } from "@tauri-apps/api/core";

export type Theme = {
  name: string;
  text: string;
  text_dim: string;
  text_muted: string;
  rule: string;
  rule_faint: string;
  ground: string;
  ramp: string[];
  mono: string;
};

export type Proc = { pid: number; name: string; cpu: number; mem: number };

export type System = {
  cores: number[][];
  mem_used: number;
  mem_total: number;
  swap_used: number;
  swap_total: number;
  procs: Proc[];
  temp_c: number | null;
  tasks: number;
  uptime_secs: number;
  vendor: string;
  model: string;
  chassis: string;
  cpu_model: string;
  cores_total: number;
  host: string;
};

export type Network = {
  up: boolean;
  iface: string;
  ipv4: string;
  gateway: string;
  dns: string;
  mac: string;
  mtu: string;
  rx_rate: number;
  tx_rate: number;
  rx_total: number;
  tx_total: number;
  rx_hist: number[];
  tx_hist: number[];
  established: number;
  listening: number;
  peers: { addr: string; count: number }[];
  listeners: string[];
};

export type FsKind = "up" | "dir" | "file" | "link" | "other";
export type FsEntry = { name: string; kind: FsKind; size: number; hidden: boolean };
export type Filesystem = {
  cwd: string;
  entries: FsEntry[];
  used: number;
  total: number;
  mount: string;
};

export const getTheme = () => invoke<Theme>("theme");
export const getSystem = () => invoke<System>("system");
export const getNetwork = () => invoke<Network>("network");
export const getFilesystem = () => invoke<Filesystem>("filesystem");

export const ptySpawn = (id: string, cols: number, rows: number) =>
  invoke<void>("pty_spawn", { id, cols, rows });
export const ptyWrite = (id: string, data: string) =>
  invoke<void>("pty_write", { id, data });
export const ptyResize = (id: string, cols: number, rows: number) =>
  invoke<void>("pty_resize", { id, cols, rows });
