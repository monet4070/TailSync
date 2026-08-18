import { useEffect, useState, type CSSProperties } from "react";
import {
  previewThemeAssetSlot,
  type ResolvedThemeV2,
} from "../tailsyncClient";
import { themeV2CssPairs } from "../utils/themeV2Css";

interface ThemePackagePreviewProps {
  resolved: ResolvedThemeV2;
  path: string;
  digest: string;
  label: string;
  loadAsset?: typeof previewThemeAssetSlot;
  stateLabels?: Partial<Record<ComponentState, string>>;
}

type ComponentState = "default" | "hover" | "active" | "selected" | "disabled" | "focus";
const componentStates: ComponentState[] = ["default", "hover", "active", "selected", "disabled", "focus"];

export function ThemePackagePreview({
  resolved,
  path,
  digest,
  label,
  loadAsset = previewThemeAssetSlot,
  stateLabels = {},
}: ThemePackagePreviewProps) {
  const [assetUrls, setAssetUrls] = useState<Record<string, string>>({});
  useEffect(() => {
    let cancelled = false;
    const urls: string[] = [];
    void Promise.all(Object.entries(resolved.assetSlots).map(async ([slot, descriptor]) => {
      try {
        const bytes = await loadAsset(path, digest, slot);
        if (cancelled) return;
        const url = URL.createObjectURL(new Blob([bytes], { type: descriptor.mimeType }));
        urls.push(url);
        setAssetUrls((current) => ({ ...current, [slot]: url }));
      } catch {
        // Optional assets never prevent the isolated token preview.
      }
    }));
    return () => {
      cancelled = true;
      urls.forEach((url) => URL.revokeObjectURL(url));
    };
  }, [digest, loadAsset, path, resolved.assetSlots]);

  const style = Object.fromEntries(
    themeV2CssPairs(resolved.tokens as Record<string, unknown>),
  ) as CSSProperties;
  const stateStyle = (state: ComponentState) => ({
    background: `var(--theme-button-${state}-background, var(--bg-card))`,
    color: `var(--theme-button-${state}-foreground, var(--text-primary))`,
    borderColor: `var(--theme-button-${state}-border, var(--border))`,
    borderRadius: `var(--theme-button-${state}-radius, var(--radius-sm))`,
    padding: `var(--theme-button-${state}-padding, 6px 8px)`,
    boxShadow: `var(--theme-button-${state}-shadow, none)`,
    outline: state === "focus" ? "2px solid var(--theme-button-focus-focus-ring, var(--brand))" : undefined,
    opacity: state === "disabled" ? 0.55 : 1,
  } as CSSProperties);
  return <div className="theme-package-preview" style={style} aria-label={`${label} theme preview`}>
    <strong className="theme-package-preview-label">{label}</strong>
    <div className="theme-package-preview-bar"><span /><span /><span /></div>
    {assetUrls.logo && <img className="theme-package-preview-logo" src={assetUrls.logo} alt="" />}
    <div className="theme-package-preview-search">Search history</div>
    {assetUrls.emptyState && <img className="theme-package-preview-empty" src={assetUrls.emptyState} alt="" />}
    {assetUrls.previewPlaceholder && <img className="theme-package-preview-placeholder" src={assetUrls.previewPlaceholder} alt="" />}
    <div className="theme-package-preview-row preview-hover"><i /><b>Hover history row</b></div>
    <div className="theme-package-preview-row preview-selected"><i /><b>Selected history row</b></div>
    <div className="theme-package-preview-focus">Focused search field</div>
    <div className="theme-package-preview-states" aria-label="Button states">
      {componentStates.map((state) => (
        <div className="theme-package-preview-state" key={state}>
          <span>{stateLabels[state] ?? state}</span>
          <div className="theme-package-preview-state-sample" style={stateStyle(state)}>
            {stateLabels[state] ?? state}
          </div>
        </div>
      ))}
    </div>
  </div>;
}
