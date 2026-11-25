import { clsx, type ClassValue } from "clsx";
import { twMerge } from "tailwind-merge";

export const cn = (...i: ClassValue[]) => twMerge(clsx(i));

/** Decimal units, matching Finder and "About This Mac". */
export function bytes(n: number, digits = 1): string {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let v = n, i = 0;
  while (v >= 1000 && i < u.length - 1) { v /= 1000; i++; }
  return `${i >= 3 ? v.toFixed(digits) : Math.round(v)} ${u[i]}`;
}

export function ago(secs: number | null): string {
  if (!secs) return "";
  const d = Math.floor((Date.now() / 1000 - secs) / 86400);
  return d < 1 ? "today" : d < 30 ? `${d}d ago` : d < 365 ? `${Math.floor(d / 30)}mo ago` : `${Math.floor(d / 365)}y ago`;
}

/** "in 3 days", "in 5 months": for forecasts. */
export function inDays(d: number): string {
  if (d < 1) return "within a day";
  if (d < 60) return `in ~${Math.round(d)} days`;
  if (d < 730) return `in ~${Math.round(d / 30)} months`;
  return "in over 2 years";
}

/** Short local date-time, e.g. "Nov 26, 14:05". */
export function when(secs: number): string {
  return new Date(secs * 1000).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" });
}

export const signedBytes = (n: number) => `${n > 0 ? "+" : "−"}${bytes(Math.abs(n))}`;
