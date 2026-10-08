// The file-browser icon set.
//
// eDEX ships five icon packs (atom, devopicons, font-awesome, mfixx,
// bytesize) and picks per file extension. These are its five STRUCTURAL
// shapes — the ones that carry the panel's meaning — drawn as inline SVG so
// nothing has to be bundled or matched by extension yet. The per-language
// pack is a later pass; the silhouettes are what make the grid readable.

export function FolderIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor">
      <path d="M2 5.5A1.5 1.5 0 0 1 3.5 4h5.3c.4 0 .8.16 1.06.44L11.4 6H20.5A1.5 1.5 0 0 1 22 7.5v11a1.5 1.5 0 0 1-1.5 1.5h-17A1.5 1.5 0 0 1 2 18.5v-13Z" />
    </svg>
  );
}

export function FileIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor">
      <path d="M5 3h8.2L20 9.6V21H5V3Z" />
      <path d="M13.2 3 20 9.6h-6.8V3Z" fill="var(--ground)" />
    </svg>
  );
}

export function LinkIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2">
      <rect x="2" y="8" width="12" height="8" rx="4" />
      <rect x="10" y="8" width="12" height="8" rx="4" />
    </svg>
  );
}

export function UpIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="currentColor">
      <path d="M2 7.5A1.5 1.5 0 0 1 3.5 6h4.8l1.5 1.6H18A1.5 1.5 0 0 1 19.5 9v9A1.5 1.5 0 0 1 18 19.5H3.5A1.5 1.5 0 0 1 2 18V7.5Z" opacity=".55" />
      <path d="M12 4 18 11h-4v6h-4v-6H6l6-7Z" />
    </svg>
  );
}

export function OtherIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeDasharray="3 2.5">
      <rect x="3" y="3" width="18" height="18" rx="1.5" />
    </svg>
  );
}

export const ICONS = {
  dir: FolderIcon,
  file: FileIcon,
  link: LinkIcon,
  up: UpIcon,
  other: OtherIcon,
} as const;
