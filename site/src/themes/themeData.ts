import {
  BookOpen,
  CircuitBoard,
  Feather,
  Flower2,
  Terminal,
  type LucideIcon,
} from 'lucide-react'
import canvasAtelier from '../../../themes/canvas-atelier/theme.json'
import fluxCircuit from '../../../themes/flux-circuit/theme.json'
import ledgerArchive from '../../../themes/ledger-archive/theme.json'
import auraBloom from '../../../themes/aura-bloom/theme.json'
import monoStrict from '../../../themes/mono-strict/theme.json'

export type ThemeMode = 'light' | 'dark'

type ColorGroups = Record<string, Record<string, string>>

interface ComponentStateOverride {
  background?: string
  foreground?: string
  secondaryText?: string
  border?: string
  focusRing?: string
  icon?: string
  accent?: string
  radius?: number
  padding?: number
  spacing?: number
  typography?: { size?: number; weight?: number }
  shadow?: { radius?: number; y?: number; opacity?: number }
}

type ComponentName = 'search' | 'history' | 'section' | 'panel' | 'button' | 'input' | 'toast'
type StateName = 'default' | 'hover' | 'active' | 'selected' | 'disabled' | 'focus'

interface ThemeManifestData {
  id: string
  name: Record<string, string>
  foundation: {
    typography?: {
      ui?: { families?: string[]; size?: number; lineHeight?: number }
      display?: { families?: string[] }
      reading?: { families?: string[] }
      search?: { size?: number; useDisplayFont?: boolean }
      section?: { size?: number; uppercase?: boolean }
      history?: { size?: number }
    }
    density?: { control?: number; row?: number }
    shape?: { controlRadius?: number; surfaceRadius?: number; windowRadius?: number }
    effects?: {
      opacity?: number
      shadow?: { radius?: number; y?: number; opacity?: number }
      motion?: { fast?: number; slow?: number; easing?: string }
    }
  }
  components?: Partial<Record<ComponentName, Partial<Record<StateName, ComponentStateOverride>>>>
  light: { colors: ColorGroups }
  dark: { colors: ColorGroups }
}

interface ResolvedComponentState {
  background: string
  foreground: string
  secondaryText: string
  border: string
  focusRing: string
  icon: string
  accent: string
  radius: number
  padding: number
  spacing: number
  typography: { size: number; weight: number }
  shadow: { radius: number; y: number; opacity: number }
}

export interface ModeRender {
  vars: Record<string, string>
}

export interface ShowcaseTheme {
  slug: string
  index: string
  id: string
  packageFile: string
  packageUrl: string
  icon: LucideIcon
  nameZh: string
  nameEn: string
  tagline: string
  description: string
  traits: string[]
  displayFamily: string
  accentLight: string
  accentDark: string
  shapeLabel: string
  shadowLabel: string
  motionLabel: string
  chapterVars: Record<string, string>
  light: ModeRender
  dark: ModeRender
}

// Mirrors the Canvas baseline component tree in shared/rust-core/src/themes_v2.rs
// (`component_state` / `component_defaults`) so the mock windows render the same
// fallback values the app would resolve at runtime.
const UI_FAMILIES = ['Segoe UI Variable Text', 'Segoe UI', 'Microsoft YaHei UI', 'PingFang SC', 'sans-serif']

function ref(path: string): string {
  return `ref:${path}`
}

function baselineState(component: ComponentName, state: StateName): Record<keyof ResolvedComponentState, string | number | object> {
  const isField = component === 'search' || component === 'input'
  const background =
    state === 'hover'
      ? ref('/colors/background/hover')
      : state === 'active'
        ? ref('/colors/background/active')
        : state === 'selected'
          ? ref('/colors/accent/soft')
          : isField
            ? ref('/colors/background/input')
            : ref('/colors/background/surface')
  const disabled = state === 'disabled'
  const accent = state === 'hover' ? ref('/colors/accent/hover') : ref('/colors/accent/default')
  return {
    background,
    foreground: ref(disabled ? '/colors/text/tertiary' : '/colors/text/primary'),
    secondaryText: ref(disabled ? '/colors/text/tertiary' : '/colors/text/secondary'),
    border: ref('/colors/border/default'),
    focusRing: disabled ? ref('/colors/text/tertiary') : accent,
    icon: ref(disabled ? '/colors/text/tertiary' : '/colors/text/secondary'),
    accent: disabled ? ref('/colors/text/tertiary') : accent,
    radius: 9,
    padding: 10,
    spacing: 8,
    typography: { size: 13, weight: 400 },
    shadow: { radius: 8, y: 3, opacity: 0.1 },
  }
}

function resolveColor(value: string, colors: ColorGroups): string {
  if (!value.startsWith('ref:')) return value
  const parts = value.slice(4).split('/').filter(Boolean)
  let node: unknown = colors
  for (const part of parts.slice(1)) {
    if (typeof node !== 'object' || node === null) return value
    node = (node as Record<string, unknown>)[part]
  }
  return typeof node === 'string' ? node : value
}

function resolveComponents(
  manifest: ThemeManifestData,
  colors: ColorGroups,
): Record<ComponentName, Record<StateName, ResolvedComponentState>> {
  const names: ComponentName[] = ['search', 'history', 'section', 'panel', 'button', 'input', 'toast']
  const states: StateName[] = ['default', 'hover', 'active', 'selected', 'disabled', 'focus']
  const result = {} as Record<ComponentName, Record<StateName, ResolvedComponentState>>
  for (const name of names) {
    const stateMap = {} as Record<StateName, ResolvedComponentState>
    for (const state of states) {
      const base = baselineState(name, state)
      const override = manifest.components?.[name]?.[state] ?? {}
      const baseTypography = base.typography as { size: number; weight: number }
      const baseShadow = base.shadow as { radius: number; y: number; opacity: number }
      stateMap[state] = {
        background: resolveColor((override.background ?? base.background) as string, colors),
        foreground: resolveColor((override.foreground ?? base.foreground) as string, colors),
        secondaryText: resolveColor((override.secondaryText ?? base.secondaryText) as string, colors),
        border: resolveColor((override.border ?? base.border) as string, colors),
        focusRing: resolveColor((override.focusRing ?? base.focusRing) as string, colors),
        icon: resolveColor((override.icon ?? base.icon) as string, colors),
        accent: resolveColor((override.accent ?? base.accent) as string, colors),
        radius: override.radius ?? (base.radius as number),
        padding: override.padding ?? (base.padding as number),
        spacing: override.spacing ?? (base.spacing as number),
        typography: { ...baseTypography, ...override.typography },
        shadow: { ...baseShadow, ...override.shadow },
      }
    }
    result[name] = stateMap
  }
  return result
}

const EASING_CSS: Record<string, string> = {
  standard: 'cubic-bezier(0.2, 0, 0, 1)',
  linear: 'linear',
  easeIn: 'cubic-bezier(0.7, 0, 0.84, 0)',
  easeOut: 'cubic-bezier(0.22, 1, 0.36, 1)',
  easeInOut: 'cubic-bezier(0.65, 0, 0.35, 1)',
}

function shadowCss(shadow: { radius: number; y: number; opacity: number }): string {
  if (shadow.opacity <= 0 || shadow.radius <= 0) return 'none'
  return `0 ${shadow.y}px ${shadow.radius}px rgba(10, 9, 6, ${shadow.opacity})`
}

function buildModeVars(manifest: ThemeManifestData, mode: ThemeMode): Record<string, string> {
  const colors = manifest[mode].colors
  const foundation = manifest.foundation
  const typography = foundation.typography ?? {}
  const shape = foundation.shape ?? {}
  const effects = foundation.effects ?? {}
  const motion = effects.motion ?? {}
  const components = resolveComponents(manifest, colors)

  const search = components.search
  const history = components.history
  const button = components.button
  const toast = components.toast
  const section = components.section

  const searchSize = typography.search?.size ?? 18
  const searchFont = typography.search?.useDisplayFont ?? true
  const sectionSize = typography.section?.size ?? 23
  const sectionUppercase = typography.section?.uppercase ?? false
  const historySize = typography.history?.size ?? 13
  const displayFamilies = typography.display?.families ?? UI_FAMILIES
  const uiFamilies = typography.ui?.families ?? UI_FAMILIES

  return {
    '--mw-accent': colors.accent.default,
    '--mw-accent-hover': colors.accent.hover,
    '--mw-accent-soft': colors.accent.soft,
    '--mw-on-accent': colors.accent.onAccent,
    '--mw-canvas': colors.background.canvas,
    '--mw-surface': colors.background.surface,
    '--mw-input': colors.background.input,
    '--mw-hover-bg': colors.background.hover,
    '--mw-active-bg': colors.background.active,
    '--mw-toast-bg': colors.background.toast,
    '--mw-text': colors.text.primary,
    '--mw-text-2': colors.text.secondary,
    '--mw-text-3': colors.text.tertiary,
    '--mw-toast-text': colors.text.toast,
    '--mw-border': colors.border.default,
    '--mw-border-strong': colors.border.strong,
    '--mw-divider': colors.border.divider,
    '--mw-positive': colors.status.positive,
    '--mw-warning': colors.status.warning,
    '--mw-info': colors.status.info,
    '--mw-font-ui': uiFamilies.join(', '),
    '--mw-font-display': displayFamilies.join(', '),
    '--mw-search-font': (searchFont ? displayFamilies : uiFamilies).join(', '),
    '--mw-search-size': `${searchSize}px`,
    '--mw-section-size': `${sectionSize}px`,
    '--mw-section-case': sectionUppercase ? 'uppercase' : 'none',
    '--mw-history-size': `${historySize}px`,
    '--mw-radius-control': `${shape.controlRadius ?? 9}px`,
    '--mw-radius-surface': `${shape.surfaceRadius ?? 10}px`,
    '--mw-radius-window': `${shape.windowRadius ?? 10}px`,
    '--mw-shadow': shadowCss({
      radius: effects.shadow?.radius ?? 70,
      y: effects.shadow?.y ?? 24,
      opacity: effects.shadow?.opacity ?? 0.16,
    }),
    '--mw-motion-fast': `${motion.fast ?? 160}ms`,
    '--mw-motion-slow': `${motion.slow ?? 420}ms`,
    '--mw-easing': EASING_CSS[motion.easing ?? 'standard'] ?? EASING_CSS.standard,
    '--mw-search-bg': search.default.background,
    '--mw-search-border': search.default.border,
    '--mw-search-icon': search.default.icon,
    '--mw-search-radius': `${search.default.radius}px`,
    '--mw-search-padding': `${search.default.padding}px`,
    '--mw-search-shadow': shadowCss(search.default.shadow),
    '--mw-search-focus-border': search.focus.border,
    '--mw-search-focus-ring': search.focus.focusRing,
    '--mw-search-focus-shadow': shadowCss(search.focus.shadow),
    '--mw-history-bg': history.default.background,
    '--mw-history-fg': history.default.foreground,
    '--mw-history-text-2': history.default.secondaryText,
    '--mw-history-icon': history.default.icon,
    '--mw-history-accent': history.default.accent,
    '--mw-history-border': history.default.border,
    '--mw-history-radius': `${history.default.radius}px`,
    '--mw-history-padding': `${history.default.padding}px`,
    '--mw-history-spacing': `${history.default.spacing}px`,
    '--mw-history-shadow': shadowCss(history.default.shadow),
    '--mw-history-hover-bg': history.hover.background,
    '--mw-history-hover-shadow': shadowCss(history.hover.shadow),
    '--mw-history-sel-bg': history.selected.background,
    '--mw-history-sel-fg': history.selected.foreground,
    '--mw-history-sel-text-2': history.selected.secondaryText,
    '--mw-history-sel-icon': history.selected.icon,
    '--mw-history-sel-border': history.selected.border,
    '--mw-history-sel-accent': history.selected.accent,
    '--mw-button-fg-weight': `${button.default.typography.weight}`,
    '--mw-button-radius': `${button.default.radius}px`,
    '--mw-button-padding': `${button.default.padding}px`,
    '--mw-button-shadow': shadowCss(button.default.shadow),
    '--mw-button-hover-shadow': shadowCss(button.hover.shadow),
    '--mw-toast-radius': `${toast.default.radius}px`,
    '--mw-toast-shadow': shadowCss(toast.default.shadow),
    '--mw-section-weight': `${section.default.typography.weight}`,
    '--mw-section-spacing': `${section.default.spacing}px`,
  }
}

const REPO_RAW = 'https://github.com/monet4070/TailSync/raw/main/themes/packages'

interface ThemeMeta {
  slug: string
  index: string
  manifest: ThemeManifestData
  packageFile: string
  icon: LucideIcon
  nameZh: string
  tagline: string
  description: string
  traits: string[]
}

const THEME_META: ThemeMeta[] = [
  {
    slug: 'atelier',
    index: '01',
    manifest: canvasAtelier as ThemeManifestData,
    packageFile: 'canvas-atelier-2.0.1.tailsync-theme',
    icon: Feather,
    nameZh: '纸上工坊',
    tagline: '暖色纸面，适合长时间阅读。',
    description:
      '在 Canvas 基础上使用更暖的背景、更深的文字、更大的衬线标题和较柔和的阴影。适合喜欢纸张质感和宽松排版的用户。',
    traits: ['暖纸底色', '衬线大标题', '柔和大投影'],
  },
  {
    slug: 'circuit',
    index: '02',
    manifest: fluxCircuit as ThemeManifestData,
    packageFile: 'flux-circuit-2.0.1.tailsync-theme',
    icon: CircuitBoard,
    nameZh: '流电矩阵',
    tagline: '紧凑、清晰，信息密度更高。',
    description:
      '使用几何无衬线字体、较紧凑的间距和短促动效，青色作为重点色。适合希望一屏看到更多内容的用户。',
    traits: ['几何无衬线', '紧凑密度', '敏捷动效'],
  },
  {
    slug: 'archive',
    index: '03',
    manifest: ledgerArchive as ThemeManifestData,
    packageFile: 'ledger-archive-2.0.1.tailsync-theme',
    icon: BookOpen,
    nameZh: '绿档账房',
    tagline: '更像档案和账簿的排版。',
    description:
      '使用书籍式衬线字体、小圆角和大写章节标题，绿色用于重点状态。整体更规整，适合偏好传统文档界面的用户。',
    traits: ['账簿大写节标题', '小圆角', '书籍衬线'],
  },
  {
    slug: 'bloom',
    index: '04',
    manifest: auraBloom as ThemeManifestData,
    packageFile: 'aura-bloom-2.0.1.tailsync-theme',
    icon: Flower2,
    nameZh: '绮光绽放',
    tagline: '更大的圆角和更柔和的配色。',
    description:
      '控件和窗口使用更大的圆角，搭配玫红重点色和较柔和的阴影。界面更轻松，但文字对比度和信息层级保持不变。',
    traits: ['最大圆角', '玫红重点色', '柔和大阴影'],
  },
  {
    slug: 'strict',
    index: '05',
    manifest: monoStrict as ThemeManifestData,
    packageFile: 'mono-strict-2.0.1.tailsync-theme',
    icon: Terminal,
    nameZh: '白纸黑律',
    tagline: '只保留黑白灰和必要的状态色。',
    description:
      '使用灰阶语义色、零圆角、零阴影和等宽字体，选中项采用反白显示。装饰最少，重点放在内容和可读性上。',
    traits: ['零圆角', '全等宽字体', '反白选中'],
  },
]

function buildShowcaseTheme(meta: ThemeMeta): ShowcaseTheme {
  const { manifest } = meta
  const foundation = manifest.foundation
  const typography = foundation.typography ?? {}
  const shape = foundation.shape ?? {}
  const effects = foundation.effects ?? {}
  const shadow = effects.shadow ?? { radius: 70, y: 24, opacity: 0.16 }
  const motion = effects.motion ?? { fast: 160, slow: 420, easing: 'standard' }
  const shapeValues = [shape.controlRadius ?? 9, shape.surfaceRadius ?? 10, shape.windowRadius ?? 10]

  return {
    slug: meta.slug,
    index: meta.index,
    id: manifest.id,
    packageFile: meta.packageFile,
    packageUrl: `${REPO_RAW}/${meta.packageFile}`,
    icon: meta.icon,
    nameZh: meta.nameZh,
    nameEn: manifest.name.en,
    tagline: meta.tagline,
    description: meta.description,
    traits: meta.traits,
    displayFamily: typography.display?.families?.[0] ?? UI_FAMILIES[0],
    accentLight: manifest.light.colors.accent.default,
    accentDark: manifest.dark.colors.accent.default,
    shapeLabel:
      shapeValues[0] === 0 && shapeValues[1] === 0 && shapeValues[2] === 0
        ? '0 · 零圆角'
        : `${shapeValues[0]} / ${shapeValues[1]} / ${shapeValues[2]} px`,
    shadowLabel:
      (shadow.opacity ?? 0.16) === 0 || (shadow.radius ?? 70) === 0
        ? '无投影'
        : `${shadow.radius ?? 70}px · Y${shadow.y ?? 24} · ${Math.round((shadow.opacity ?? 0.16) * 100)}%`,
    motionLabel: `${motion.fast ?? 160} / ${motion.slow ?? 420} ms · ${motion.easing ?? 'standard'}`,
    chapterVars: {
      '--ch-bg': manifest.light.colors.background.canvas,
      '--ch-text': manifest.light.colors.text.primary,
      '--ch-text-2': manifest.light.colors.text.secondary,
      '--ch-border': manifest.light.colors.border.default,
      '--ch-accent': manifest.light.colors.accent.default,
      '--ch-accent-soft': manifest.light.colors.accent.soft,
      '--ch-on-accent': manifest.light.colors.accent.onAccent,
      '--ch-divider': manifest.light.colors.border.divider,
      '--ch-radius': `${shape.controlRadius ?? 9}px`,
      '--ch-font-display': (typography.display?.families ?? UI_FAMILIES).join(', '),
    },
    light: { vars: buildModeVars(manifest, 'light') },
    dark: { vars: buildModeVars(manifest, 'dark') },
  }
}

export const showcaseThemes: ShowcaseTheme[] = THEME_META.map(buildShowcaseTheme)
