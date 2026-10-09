// What the panels draw, given numbers.
//
// The point of these is the failure the gate could not see: a panel that
// renders nothing, or renders a dash where a value should be. khadi-hud had
// exactly this bug once — three of four panels dead while the gate said
// 71/71 — and the rewrite reopened the hole by moving the panels behind a
// webview. These close it without needing one.

import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { CpuInfo, Hardware, litCells, RamWatcher, SysInfo, TopList } from "../components/LeftColumn";
import { ConnInfo, NetStat } from "../components/RightColumn";
import { FilesystemPanel } from "../components/Filesystem";
import type { Filesystem, Network, System } from "../lib/ipc";

afterEach(cleanup);

const sys: System = {
  cores: [[10, 20, 30], [40, 50, 60]],
  mem_used: 4 * 1024 ** 3,
  mem_total: 8 * 1024 ** 3,
  swap_used: 0,
  swap_total: 2 * 1024 ** 3,
  procs: [
    { pid: 1234, name: "khadi-shell", cpu: 12.5, mem: 200 * 1024 ** 2 },
    { pid: 5678, name: "Hyprland", cpu: 3.25, mem: 150 * 1024 ** 2 },
  ],
  temp_c: 62,
  tasks: 257,
  uptime_secs: 4191,
  vendor: "ASUSTeK COMPUTER",
  model: "G551JK",
  chassis: "Notebook",
  cpu_model: "Intel Core i5-4200H",
  cores_total: 4,
  host: "batcore-home",
};

const net: Network = {
  up: true,
  iface: "wlp3s0",
  ipv4: "192.168.1.20",
  gateway: "192.168.1.1",
  dns: "1.1.1.1",
  mac: "aa:bb:cc:dd:ee:ff",
  mtu: "1500",
  rx_rate: 2048,
  tx_rate: 512,
  rx_total: 1024 ** 3,
  tx_total: 1024 ** 2,
  rx_hist: [1, 2, 3, 4],
  tx_hist: [4, 3, 2, 1],
  established: 7,
  listening: 3,
  peers: [{ addr: "140.82.112.25:443", count: 2 }],
  listeners: ["*:22"],
};

const fs: Filesystem = {
  cwd: "/home/khadi",
  entries: [
    { name: "UP", kind: "up", size: 0, hidden: false },
    { name: ".config", kind: "dir", size: 0, hidden: true },
    { name: "notes.md", kind: "file", size: 120, hidden: false },
    { name: "link", kind: "link", size: 0, hidden: false },
  ],
  used: 5 * 1024 ** 3,
  total: 20 * 1024 ** 3,
  mount: "/home",
};

describe("SysInfo", () => {
  it("shows uptime as eDEX's H:MM:SS, not a dash", () => {
    render(<SysInfo s={sys} />);
    expect(screen.getByText("1:09:51")).toBeTruthy();
    expect(screen.getByText("257")).toBeTruthy();
  });
  it("degrades to dashes rather than crashing with no data", () => {
    render(<SysInfo s={null} />);
    expect(screen.getAllByText("—").length).toBeGreaterThan(0);
  });
});

describe("Hardware", () => {
  it("shows all three DMI fields", () => {
    render(<Hardware s={sys} />);
    for (const v of ["ASUSTeK COMPUTER", "G551JK", "Notebook"]) {
      expect(screen.getByText(v)).toBeTruthy();
    }
  });
});

describe("CpuInfo", () => {
  it("draws one row per core PAIR, as eDEX does", () => {
    render(<CpuInfo s={sys} />);
    expect(screen.getByText("#1-2")).toBeTruthy();
    expect(screen.queryByText("#3-4")).toBeNull(); // only two cores given
    expect(screen.getByText("Intel Core i5-4200H")).toBeTruthy();
  });
  it("reports the host and core count", () => {
    render(<CpuInfo s={sys} />);
    expect(screen.getByText("batcore-home")).toBeTruthy();
    expect(screen.getByText("62°C")).toBeTruthy();
  });
});

describe("RamWatcher", () => {
  it("states the real total, not a placeholder", () => {
    render(<RamWatcher s={sys} />);
    expect(screen.getByText("USING 4.0 OUT OF 8.0 GIB")).toBeTruthy();
  });
});

describe("litCells", () => {
  it("lights exactly the fraction it was given", () => {
    // The claim the block makes is "N of M in use". Spreading them must not
    // change N.
    for (const frac of [0, 0.25, 0.5, 0.731, 1]) {
      const lit = litCells(frac, 368).filter(Boolean).length;
      expect(lit).toBe(Math.round(frac * 368));
    }
  });
  it("spreads them instead of packing the top rows", () => {
    // Packed, a machine at 20% lit only the first row and the block looked
    // broken. Half the lit cells should fall in each half of the grid.
    const lit = litCells(0.5, 368);
    const firstHalf = lit.slice(0, 184).filter(Boolean).length;
    expect(firstHalf).toBeGreaterThan(60);
    expect(firstHalf).toBeLessThan(124);
  });
});

describe("TopList", () => {
  it("shows process names and formatted memory", () => {
    render(<TopList s={sys} />);
    expect(screen.getByText("khadi-shell")).toBeTruthy();
    expect(screen.getByText("12.5%")).toBeTruthy();
    expect(screen.getByText("200.0MiB")).toBeTruthy();
  });
});

describe("NetStat", () => {
  it("shows the interface and its addresses", () => {
    render(<NetStat n={net} />);
    expect(screen.getByText("ONLINE")).toBeTruthy();
    expect(screen.getByText("192.168.1.20")).toBeTruthy();
    expect(screen.getByText("aa:bb:cc:dd:ee:ff")).toBeTruthy();
  });
  it("says OFFLINE rather than going blank", () => {
    render(<NetStat n={{ ...net, up: false }} />);
    expect(screen.getByText("OFFLINE")).toBeTruthy();
  });
});

describe("ConnInfo", () => {
  it("shows rates, totals and live peers", () => {
    render(<ConnInfo n={net} />);
    expect(screen.getByText("DOWN 2.0 K/s")).toBeTruthy();
    expect(screen.getByText("7 ESTABLISHED")).toBeTruthy();
    expect(screen.getByText("140.82.112.25:443")).toBeTruthy();
  });
});

describe("FilesystemPanel", () => {
  it("lists the directory and the mount bar", () => {
    render(<FilesystemPanel fs={fs} />);
    expect(screen.getByText("notes.md")).toBeTruthy();
    // The cwd is in the header; the mount point is in the bar beneath it.
    expect(screen.getByText("/home/khadi")).toBeTruthy();
    expect(screen.getByText("/home")).toBeTruthy();
    expect(screen.getByText(/used 25%/)).toBeTruthy();
  });
  it("gives every entry kind an icon", () => {
    const { container } = render(<FilesystemPanel fs={fs} />);
    expect(container.querySelectorAll("svg").length).toBe(fs.entries.length);
  });
});
