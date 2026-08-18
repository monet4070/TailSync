import {
  installThemeV2,
  updateThemeV2,
  validateThemeV2,
  type ResolvedThemeV2,
  type ThemeDiagnosticV2,
  type ThemeV2Descriptor,
  type ThemeValidationV2,
  type UpdateThemeOptions,
} from "../tailsyncClient";

export interface ThemePackagePreviews {
  light: ResolvedThemeV2;
  dark: ResolvedThemeV2;
  highContrastLight: ResolvedThemeV2;
  highContrastDark: ResolvedThemeV2;
}

export type ThemePackageOperation =
  | { kind: "install" }
  | { kind: "update"; themeId: string; installedVersion: string };

export type ThemeVersionRelation = "upgrade" | "same" | "downgrade";

export interface PendingThemePackage {
  path: string;
  digest: string;
  previews: ThemePackagePreviews;
  diagnostics: ThemeDiagnosticV2[];
  candidateVersion: string;
  versionRelation?: ThemeVersionRelation;
  operation: ThemePackageOperation;
}

interface ParsedVersion { core: [number, number, number]; prerelease?: string[] }

function parseThemeVersion(version: string): ParsedVersion | undefined {
  const match = /^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$/.exec(version);
  if (!match) return undefined;
  const prerelease = match[4]?.split(".");
  if (prerelease?.some((part) => /^\d+$/.test(part) && part.length > 1 && part.startsWith("0"))) return undefined;
  return { core: [Number(match[1]), Number(match[2]), Number(match[3])], prerelease };
}

export function compareThemeVersions(candidate: string, installed: string): ThemeVersionRelation {
  const left = parseThemeVersion(candidate);
  const right = parseThemeVersion(installed);
  if (!left || !right) throw new Error("Theme version is not valid SemVer");
  for (let index = 0; index < left.core.length; index += 1) {
    if (left.core[index] !== right.core[index]) return left.core[index] > right.core[index] ? "upgrade" : "downgrade";
  }
  if (!left.prerelease && !right.prerelease) return "same";
  if (!left.prerelease) return "upgrade";
  if (!right.prerelease) return "downgrade";
  const count = Math.max(left.prerelease.length, right.prerelease.length);
  for (let index = 0; index < count; index += 1) {
    const a = left.prerelease[index];
    const b = right.prerelease[index];
    if (a === undefined) return "downgrade";
    if (b === undefined) return "upgrade";
    if (a === b) continue;
    const aNumeric = /^\d+$/.test(a);
    const bNumeric = /^\d+$/.test(b);
    if (aNumeric && bNumeric) {
      if (a.length !== b.length) return a.length > b.length ? "upgrade" : "downgrade";
      return a > b ? "upgrade" : "downgrade";
    }
    if (aNumeric !== bNumeric) return aNumeric ? "downgrade" : "upgrade";
    return a > b ? "upgrade" : "downgrade";
  }
  return "same";
}

export function updateOptionsFor(pending: PendingThemePackage): UpdateThemeOptions {
  if (pending.operation.kind !== "update") return {};
  if (pending.versionRelation === "same") return { allowSameVersion: true };
  if (pending.versionRelation === "downgrade") return { allowDowngrade: true };
  return {};
}

type ValidateTheme = (
  path: string,
  mode: "light" | "dark",
  highContrast?: boolean,
) => Promise<ThemeValidationV2>;

export async function validateThemePackageForPreview(
  path: string,
  operation: ThemePackageOperation,
  validate: ValidateTheme = validateThemeV2,
): Promise<PendingThemePackage> {
  const [light, dark, highContrastLight, highContrastDark] = await Promise.all([
    validate(path, "light"),
    validate(path, "dark"),
    validate(path, "light", true),
    validate(path, "dark", true),
  ]);
  const validations = [light, dark, highContrastLight, highContrastDark];
  const diagnostics = validations.flatMap((validation) => validation.diagnostics);
  const digest = light.digest;
  const candidateVersion = light.candidateVersion;
  if (
    validations.some((validation) => !validation.valid)
    || !digest
    || validations.some((validation) => validation.digest !== digest)
    || !candidateVersion
    || validations.some((validation) => validation.candidateVersion !== candidateVersion)
    || !light.preview
    || !dark.preview
    || !highContrastLight.preview
    || !highContrastDark.preview
  ) {
    throw diagnostics.find((diagnostic) => diagnostic.severity === "error")
      ?? diagnostics[0]
      ?? new Error("Invalid theme package");
  }
  if (
    operation.kind === "update"
    && validations.some((validation) => validation.preview?.id !== operation.themeId)
  ) {
    throw new Error("The update package id does not match the installed theme");
  }
  return {
    path,
    digest,
    previews: {
      light: light.preview,
      dark: dark.preview,
      highContrastLight: highContrastLight.preview,
      highContrastDark: highContrastDark.preview,
    },
    diagnostics,
    candidateVersion,
    versionRelation: operation.kind === "update"
      ? compareThemeVersions(candidateVersion, operation.installedVersion)
      : undefined,
    operation,
  };
}

interface ApplyThemePackageDependencies {
  install?: typeof installThemeV2;
  update?: typeof updateThemeV2;
  refresh: () => Promise<void>;
}

export async function applyThemePackageOperation(
  pending: PendingThemePackage,
  dependencies: ApplyThemePackageDependencies,
): Promise<ThemeV2Descriptor> {
  const descriptor = pending.operation.kind === "install"
    ? await (dependencies.install ?? installThemeV2)(pending.path, pending.digest)
    : await (dependencies.update ?? updateThemeV2)(pending.path, pending.digest, updateOptionsFor(pending));
  await dependencies.refresh();
  return descriptor;
}
