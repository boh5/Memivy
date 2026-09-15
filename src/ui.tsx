const paths: Record<string, string> = {
  sidebar: "M3 4h18v16H3zM9 4v16",
  plus: "M12 5v14M5 12h14",
  mic: "M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0zM5 10v2a7 7 0 0 0 14 0v-2M12 19v3M8 22h8",
  download: "M12 3v12m-4-4 4 4 4-4M4 17v4h16v-4",
  play: "m8 4 12 8-12 8z",
  arrow: "M12 19V5m-6 6 6-6 6 6",
  chevron: "m9 5 7 7-7 7",
  close: "m6 6 12 12M6 18 18 6",
  desktop: "M3 4h18v13H3zM8 21h8M12 17v4",
  database: "M21 5c0 2-4 3-9 3S3 7 3 5s4-3 9-3 9 1 9 3ZM3 5v14c0 2 4 3 9 3s9-1 9-3V5M3 12c0 2 4 3 9 3s9-1 9-3",
  link: "M10 13a5 5 0 0 0 7 0l3-3a5 5 0 0 0-7-7l-2 2M14 11a5 5 0 0 0-7 0l-3 3a5 5 0 0 0 7 7l2-2",
  lock: "M5 10h14v11H5zM8 10V6a4 4 0 0 1 8 0v4M12 14v3",
  back: "m14 5-7 7 7 7",
  search: "M21 21l-5-5M18 10a8 8 0 1 1-16 0 8 8 0 0 1 16 0",
  book: "M4 4h6c2 0 2 2 2 2s0-2 2-2h6v16h-6c-2 0-2 1-2 1s0-1-2-1H4zM12 6v15",
  pencil: "m15 4 5 5M4 20l5-1L20 8a2 2 0 0 0-5-5L4 14z",
  wand: "m5 20 12-12-3-3L2 17zM11 8l3 3M19 2v4M17 4h4M20 13v4M18 15h4M7 2v4M5 4h4",
  ellipsis: "M5 12h.01M12 12h.01M19 12h.01",
  chat: "M5 4h14a2 2 0 0 1 2 2v10a2 2 0 0 1-2 2H9l-6 4V6a2 2 0 0 1 2-2z",
  spark: "m12 3 2.6 6.4L21 12l-6.4 2.6L12 21l-2.6-6.4L3 12l6.4-2.6z",
  settings:
    "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8M12 2v3m0 14v3M2 12h3m14 0h3M5 5l2 2m10 10 2 2M5 19l2-2M17 7l2-2",
  expand: "M14 3h7v7M21 3l-8 8M10 21H3v-7m0 7 8-8",
  folder: "M3 6h7l2 2h9v12H3zM3 6V4h7l2 2h9v2",
  pin: "m8 3 8 0-1 7 3 3v2H6v-2l3-3zM12 15v7",
  check: "m5 12 4 4L19 6",
  undo: "M8 4 3 9l5 5M3 9h11a6 6 0 0 1 0 12",
  stop: "M6 6h12v12H6z",
  leaf: "M19 3C6 2 2 8 5 15c4 7 16 2 14-12ZM5 20 15 8",
  history: "M3 11a9 9 0 1 1 2 7M3 4v7h7M12 7v5l3 2",
  refresh: "M3 10a9 9 0 0 1 15-5l3 3M21 3v5h-5M21 14a9 9 0 0 1-15 5l-3-3M3 21v-5h5",
  trash: "M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7",
  note: "M5 3h10l4 4v14H5zM14 3v5h5M9 12h6M9 16h6",
};
export function Icon({ name, size = 18 }: { name: string; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.65"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[name] || paths.spark} />
    </svg>
  );
}
