// Which display arrays belong to the scan on screen (Codex review of the assembled app, finding 1).
//
// A scan's arrays depend on the scan (its id carries the session generation in the desktop app, so a new
// session never reuses an old id), its input bytes and the class it was analysed under (the transfer). The
// chart smoothing is a separate part of the request key: arrays of the same scan at the previous smoothing stay
// on screen until the new ones land, but arrays of another scan, input or class are never shown.

import type { ScanResult } from './types';

type Identity = Pick<ScanResult, 'scanId' | 'inputSha256' | 'instrumentClass'>;

/** The identity the display arrays depend on (null: nothing to draw). */
export function chartIdentity(s: Identity | null | undefined): string | null {
  if (!s) return null;
  return `${s.scanId}|${s.inputSha256 ?? ''}|${s.instrumentClass}`;
}

/** The request key: identity + chart smoothing. */
export function chartKey(identity: string, smoothing: number): string {
  return `${identity}|${smoothing}`;
}

/** Latest request wins: a response is used only if no newer request was started since. */
export class LatestOnly {
  private n = 0;
  begin(): number {
    return ++this.n;
  }
  isCurrent(ticket: number): boolean {
    return ticket === this.n;
  }
}

/** Held arrays and what they were fetched for. */
export interface HeldViews<V> {
  identity: string;
  key: string;
  views: V;
}

/** The arrays to show for the scan on screen: only arrays fetched for its identity. */
export function shownViews<V>(held: HeldViews<V> | null, identity: string | null): V | null {
  return held && identity !== null && held.identity === identity ? held.views : null;
}
