// A firmware file drawn as a page with a .syx tag: the drop zones on the patch and emulator pages.
export default function SyxFile() {
  return (
    <svg class="syx-file" viewBox="0 0 56 66" aria-hidden="true">
      <path
        d="M6 3H36L52 19V61a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2Z"
        fill="var(--file-icon-fill)"
        stroke="var(--file-icon-stroke)"
        stroke-width="1.5"
      />
      <path d="M36 3V17a2 2 0 0 0 2 2H52" fill="none" stroke="var(--file-icon-stroke)" stroke-width="1.5" />
      <rect x="10" y="40" width="34" height="14" rx="3" fill="var(--action)" />
      <text x="27" y="50.5" text-anchor="middle" class="syx-label">
        .syx
      </text>
      <path
        d="M12 27h8M23 27h14M12 33h18"
        stroke="var(--control-hover-outline)"
        stroke-width="2"
        stroke-linecap="round"
      />
    </svg>
  );
}
