import { dismissOnboarding, onboardingSuppressed, openOnboardingWindow, suppressOnboarding } from "../tailsyncClient";
import guide from "../../../shared/onboarding/guide.json";
export type TourWindow = "connections" | "history" | "settings";
export type TourState = { active: boolean; index: number };
export const TOUR_KEY = "tailsync-guided-tour-v2";
export const TOUR_EVENT = "tailsync-guided-tour-changed";
export const TOUR_SUPPRESSED_KEY = "tailsync-guided-tour-suppressed";
export const TOUR_SUPPRESSED_EVENT = "tailsync-guided-tour-suppressed-changed";
export function readTour(): TourState {
  try {
    const value = JSON.parse(localStorage.getItem(TOUR_KEY) ?? "null");
    if (value && typeof value.active === "boolean" && Number.isInteger(value.index)
      && value.index >= 0 && value.index < guide.steps.length) return value;
  } catch { /* Keep this window usable if storage is unavailable. */ }
  return { active: false, index: 0 };
}
export function saveTour(value: TourState) {
  localStorage.setItem(TOUR_KEY, JSON.stringify(value));
  window.dispatchEvent(new Event(TOUR_EVENT));
}
export async function startTour() {
  if (await readTourSuppressed()) return;
  const previous = readTour();
  saveTour({ active: true, index: 0 });
  try { await openOnboardingWindow(); }
  catch (error) { saveTour(previous); throw error; }
}
export async function moveTour(index: number, openWindow: (window: TourWindow) => Promise<unknown>) {
  const previous = readTour();
  const target = guide.steps[index];
  if (!target) return;
  saveTour({ active: true, index });
  try { await openWindow(target.window as TourWindow); }
  catch (error) { saveTour(previous); throw error; }
}
export async function endTour() {
  await dismissOnboarding();
  saveTour({ ...readTour(), active: false });
}

export async function endTourPermanently() {
  await suppressOnboarding();
  try { localStorage.setItem(TOUR_SUPPRESSED_KEY, "1"); } catch { }
  window.dispatchEvent(new Event(TOUR_SUPPRESSED_EVENT));
  saveTour({ ...readTour(), active: false });
}

export async function readTourSuppressed(): Promise<boolean> {
  // Native state knows the installation identity. A WebView cache can outlive
  // uninstall and must never override the result for a fresh installation.
  const suppressed = Boolean(await onboardingSuppressed());
  try {
    if (suppressed) localStorage.setItem(TOUR_SUPPRESSED_KEY, "1");
    else localStorage.removeItem(TOUR_SUPPRESSED_KEY);
  } catch { }
  return suppressed;
}
