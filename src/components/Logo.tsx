/**
 * TokBar logo — rising bars with a floating token dot, matching the app
 * icon. Background follows the theme accent (--primary).
 */
export function Logo({ size = 32 }: { size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 44 44"
      className="shrink-0"
      aria-hidden
    >
      <defs>
        <linearGradient id="tokbar-logo-shade" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0%" stopColor="#ffffff" stopOpacity="0.22" />
          <stop offset="100%" stopColor="#000000" stopOpacity="0.16" />
        </linearGradient>
      </defs>
      <rect width="44" height="44" rx="10.5" fill="var(--primary)" />
      <rect width="44" height="44" rx="10.5" fill="url(#tokbar-logo-shade)" />
      <rect x="9.5" y="24" width="6" height="11.5" rx="3" fill="#ffffff" />
      <rect x="19" y="18.5" width="6" height="17" rx="3" fill="#ffffff" />
      <rect x="28.5" y="13" width="6" height="22.5" rx="3" fill="#ffffff" />
      <circle cx="31.5" cy="8.6" r="2.7" fill="#ffffff" />
    </svg>
  );
}
