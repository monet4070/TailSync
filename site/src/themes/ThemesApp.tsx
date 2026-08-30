import { useEffect, type CSSProperties } from 'react'
import {
  ArrowLeft,
  ArrowUpRight,
  Check,
  Download,
  Package,
  Settings2,
  SwatchBook,
} from 'lucide-react'
import { MockWindow } from './MockWindow'
import { showcaseThemes, type ShowcaseTheme } from './themeData'

const GITHUB_URL = 'https://github.com/monet4070/TailSync'
const THEMES_TREE_URL = `${GITHUB_URL}/tree/main/themes`

function Chapter({ theme }: { theme: ShowcaseTheme }) {
  const Icon = theme.icon
  return (
    <section
      className={`chapter chapter--${theme.slug}`}
      id={theme.slug}
      style={theme.chapterVars as CSSProperties}
      data-reveal
    >
      <div className="chapter-head">
        <div className="chapter-title-block">
          <span className="chapter-index">{theme.index}</span>
          <h2 className="chapter-name">{theme.nameZh}</h2>
          <span className="chapter-en">{theme.nameEn}</span>
        </div>
        <Icon className="chapter-motif" strokeWidth={1} aria-hidden="true" />
      </div>

      <div className="chapter-body">
        <div className="chapter-meta">
          <p className="chapter-tagline">{theme.tagline}</p>
          <p className="chapter-desc">{theme.description}</p>

          <ul className="chapter-traits">
            {theme.traits.map((trait) => (
              <li key={trait}>{trait}</li>
            ))}
          </ul>

          <dl className="chapter-specs">
            <div className="chapter-spec">
              <dt>主色 / ACCENT</dt>
              <dd>
                <i className="spec-swatch" style={{ background: theme.accentLight }} />
                {theme.accentLight}
                <i className="spec-swatch" style={{ background: theme.accentDark }} />
                {theme.accentDark}
              </dd>
            </div>
            <div className="chapter-spec">
              <dt>圆角 / SHAPE</dt>
              <dd>{theme.shapeLabel}</dd>
            </div>
            <div className="chapter-spec">
              <dt>投影 / SHADOW</dt>
              <dd>{theme.shadowLabel}</dd>
            </div>
            <div className="chapter-spec">
              <dt>动效 / MOTION</dt>
              <dd>{theme.motionLabel}</dd>
            </div>
            <div className="chapter-spec">
              <dt>展示字体 / DISPLAY</dt>
              <dd>{theme.displayFamily}</dd>
            </div>
          </dl>

          <a
            className="chapter-cta"
            href={theme.packageUrl}
            target="_blank"
            rel="noreferrer"
          >
            <Download size={15} />
            获取主题包
            <ArrowUpRight size={13} />
          </a>
        </div>

        <div className="chapter-stage">
          <MockWindow render={theme.light} mode="light" className="stage-light" />
          <MockWindow render={theme.dark} mode="dark" className="stage-dark" />
        </div>
      </div>
    </section>
  )
}

export function ThemesApp() {
  useEffect(() => {
    const elements = [...document.querySelectorAll<HTMLElement>('[data-reveal]')]
    const observer = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.setAttribute('data-visible', 'true')
            observer.unobserve(entry.target)
          }
        })
      },
      { threshold: 0.08 },
    )
    elements.forEach((element) => observer.observe(element))
    return () => observer.disconnect()
  }, [])

  return (
    <div className="atelier-page">
      <header className="atelier-topbar">
        <a className="atelier-home" href="/">
          <ArrowLeft size={15} />
          <span>TAILSYNC</span>
        </a>
        <span className="atelier-topbar-tag">
          <SwatchBook size={13} />
          THEME ATELIER / V2
        </span>
        <a className="atelier-source" href={THEMES_TREE_URL} target="_blank" rel="noreferrer">
          主题包源码
          <ArrowUpRight size={13} />
        </a>
      </header>

      <main>
        <section className="atelier-hero">
          <div className="atelier-hero-kicker">
            <span className="atelier-live-dot" />
            THEME V2 · 05 THEMES · LIGHT / DARK
          </div>
          <h1 className="atelier-title">
            选择一套更顺手的界面
            <br />
            <span>五种主题都能直接导入。</span>
          </h1>
          <p className="atelier-lead">
            这里提供 Canvas、Flux、Ledger、Aura 和 Mono 五套主题的扩展版本。
            每套都包含浅色与深色配色、字体、圆角、阴影和组件状态。下载
            .tailsync-theme 文件后，可以直接在 TailSync 设置中导入。
          </p>

          <nav className="atelier-index" aria-label="主题索引">
            {showcaseThemes.map((theme) => {
              const Icon = theme.icon
              return (
                <a key={theme.slug} href={`#${theme.slug}`} className="atelier-index-item">
                  <span className="atelier-index-top">
                    <span className="atelier-index-no">{theme.index}</span>
                    <i
                      className="atelier-index-dot"
                      style={{ background: theme.accentLight }}
                    />
                  </span>
                  <span className="atelier-index-name">{theme.nameZh}</span>
                  <span className="atelier-index-en">
                    <Icon size={13} />
                    {theme.nameEn}
                  </span>
                </a>
              )
            })}
          </nav>
        </section>

        <div className="atelier-marquee" aria-hidden="true">
          <div className="atelier-marquee-track">
            {[0, 1].map((copy) => (
              <div className="atelier-marquee-group" key={copy}>
                {showcaseThemes.map((theme) => (
                  <span key={`${copy}-${theme.slug}`}>
                    {theme.nameEn}
                    <i style={{ background: theme.accentLight }} />
                  </span>
                ))}
              </div>
            ))}
          </div>
        </div>

        {showcaseThemes.map((theme) => (
          <Chapter key={theme.slug} theme={theme} />
        ))}

        <section className="import-section" id="import" data-reveal>
          <div className="import-inner">
            <div className="import-head">
              <span className="import-eyebrow">IMPORT / 30 秒</span>
              <h2>下载后，在设置里导入</h2>
              <p>
                主题包只包含颜色、字体、圆角等配置，不会执行代码，也不会联网。
                导入前会由 Rust Core 校验，选择结果只保存在当前设备。
              </p>
            </div>

            <ol className="import-steps">
              <li>
                <span className="import-step-icon">
                  <Download size={18} />
                </span>
                <strong>下载文件</strong>
                <p>从上方主题介绍或下方清单下载 .tailsync-theme 文件。</p>
              </li>
              <li>
                <span className="import-step-icon">
                  <Settings2 size={18} />
                </span>
                <strong>在 TailSync 中导入</strong>
                <p>打开“设置 → 外观 → 导入主题包”，选择刚下载的文件。</p>
              </li>
              <li>
                <span className="import-step-icon">
                  <Check size={18} />
                </span>
                <strong>预览后使用</strong>
                <p>确认四种预览状态没有问题后安装，选中主题即可应用。</p>
              </li>
            </ol>

            <div className="package-table" role="table" aria-label="增强主题包清单">
              <div className="package-row package-row-head" role="row">
                <span>主题</span>
                <span>主题 ID</span>
                <span>模式</span>
                <span>文件</span>
              </div>
              {showcaseThemes.map((theme) => {
                const Icon = theme.icon
                return (
                  <a
                    key={theme.slug}
                    className="package-row"
                    role="row"
                    href={theme.packageUrl}
                    target="_blank"
                    rel="noreferrer"
                  >
                    <span className="package-name">
                      <i style={{ background: theme.accentLight }} />
                      <Icon size={15} />
                      {theme.nameZh} · {theme.nameEn}
                    </span>
                    <span className="package-id">{theme.id}</span>
                    <span className="package-modes">LIGHT / DARK</span>
                    <span className="package-file">
                      <Package size={14} />
                      {theme.packageFile}
                      <ArrowUpRight size={12} />
                    </span>
                  </a>
                )
              })}
            </div>

            <p className="import-note">
              每个主题包通过 <code>extends: builtin:canvas@1</code> 继承基础配置，只覆盖需要调整的令牌。
              文件不超过 1.4 KB，校验、安装和回滚都由共享 Core 处理。
            </p>
          </div>
        </section>
      </main>

      <footer className="atelier-footer">
        <span>TAILSYNC / THEME ATELIER</span>
        <a href="/">返回主站</a>
        <span>MIT LICENSE</span>
      </footer>
    </div>
  )
}
