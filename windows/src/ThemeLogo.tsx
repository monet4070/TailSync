import { ClipboardCopy } from "lucide-react";

export function ThemeLogo() {
  return (
    <span className="titlebar-logo theme-logo" aria-hidden="true">
      <ClipboardCopy className="theme-logo-glyph" strokeWidth={1.9} />
    </span>
  );
}
