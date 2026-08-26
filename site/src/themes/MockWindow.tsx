import {
  Check,
  ClipboardCopy,
  File,
  FileText,
  Image as ImageIcon,
  Search,
  Wifi,
} from 'lucide-react'
import type { CSSProperties } from 'react'
import { WINDOWS_INSTALLER_NAME } from '../product'
import type { ModeRender, ThemeMode } from './themeData'

interface MockWindowProps {
  render: ModeRender
  mode: ThemeMode
  className?: string
}

const HISTORY_ITEMS = [
  { icon: FileText, title: 'themeV2Css.ts — 令牌映射完成', meta: '文本 · 09:41', selected: false },
  { icon: ImageIcon, title: 'tailsync-theme-comparison.png', meta: '图片 · 09:32', selected: true },
  { icon: File, title: WINDOWS_INSTALLER_NAME, meta: '文件 · 08:57', selected: false },
]

export function MockWindow({ render, mode, className }: MockWindowProps) {
  return (
    <div
      className={`mw mw-${mode}${className ? ` ${className}` : ''}`}
      style={render.vars as CSSProperties}
      data-mode={mode === 'light' ? 'LIGHT' : 'DARK'}
      aria-hidden="true"
    >
      <div className="mw-titlebar">
        <span className="mw-logo">
          <ClipboardCopy size={12} strokeWidth={2.2} />
        </span>
        <span className="mw-title">TailSync</span>
        <span className="mw-titlebar-meta">
          <Wifi size={11} />
          LAN · 直连
        </span>
      </div>

      <div className="mw-search">
        <Search size={16} className="mw-search-icon" />
        <span className="mw-search-placeholder">搜索剪贴板历史</span>
        <span className="mw-search-hint">Ctrl K</span>
      </div>

      <div className="mw-section">
        <span className="mw-section-label">今天</span>
        <span className="mw-section-rule" />
        <span className="mw-section-count">3 条</span>
      </div>

      <div className="mw-list">
        {HISTORY_ITEMS.map((item) => (
          <div
            key={item.title}
            className={item.selected ? 'mw-item is-selected' : 'mw-item'}
          >
            <span className="mw-item-icon">
              <item.icon size={14} />
            </span>
            <span className="mw-item-copy">
              <strong>{item.title}</strong>
              <small>{item.meta}</small>
            </span>
            {item.selected ? <Check size={13} className="mw-item-check" /> : null}
          </div>
        ))}
      </div>

      <div className="mw-footerbar">
        <span className="mw-button">
          <Check size={13} strokeWidth={2.4} />
          同步完成
        </span>
        <span className="mw-status">
          <i className="mw-status-dot" />3 台设备在线
        </span>
      </div>

      <div className="mw-toast">
        <Check size={13} strokeWidth={2.4} />
        已写入 Mac 的剪贴板
      </div>
    </div>
  )
}
