#!/usr/bin/env python3
from __future__ import annotations

import argparse
import hashlib
import html
import ipaddress
import json
import os
import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit, urlunsplit

REPO_ROOT = Path(__file__).resolve().parents[1]

TASK_ID = "TAILSYNC_R4_FIXTURES_20261009_HIGH"
SCHEMA = "tailsync.rich-text-r4-fixtures"
SCHEMA_VERSION = 1
GENERATOR = "scripts/rich-text-r4-fixtures.py"
DOCUMENTATION_BASE = "https://example.com/tailsync-rich-text-r4/"
IMAGE_DIRECTORY = "images"
MANIFEST_NAME = "manifest.json"
UNKNOWN = "UNKNOWN"
EXIT_USAGE = 2
EXIT_FAILURE = 1

PRIVATE_HOST_SUFFIXES = (
    ".localhost",
    ".local",
    ".localdomain",
    ".internal",
    ".intranet",
    ".lan",
    ".home",
    ".home.arpa",
    ".corp",
    ".private",
    ".test",
    ".example",
    ".invalid",
)

IMAGE_SOURCES = (
    {
        "name": "transparent.png",
        "encoding": "png",
        "content_type": "image/png",
        "source": "shared/tailsync-protocol/tests/fixtures/images/transparent.png",
        "alt": "图一：PNG 透明图",
        "caption": "图一（顺序 1）：PNG（transparent.png）",
    },
    {
        "name": "color.jpg",
        "encoding": "jpeg",
        "content_type": "image/jpeg",
        "source": "shared/tailsync-protocol/tests/fixtures/images/color.jpg",
        "alt": "图二：JPEG 彩色图",
        "caption": "图二（顺序 2）：JPEG（color.jpg）",
    },
    {
        "name": "static.webp",
        "encoding": "webp",
        "content_type": "image/webp",
        "source": "shared/tailsync-protocol/tests/fixtures/images/static.webp",
        "alt": "图三：静态 WebP 图",
        "caption": "图三（顺序 3）：静态 WebP（static.webp）",
    },
)

MISSING_IMAGE_PATH = "images/absent-color.jpg"
MISSING_IMAGE_ENCODING = "jpeg"
MISSING_IMAGE_ALT = "图二：JPEG 故意缺失"
MISSING_IMAGE_CAPTION = "图二（顺序 2，故意缺失）：absent-color.jpg 不随目录发布，加载失败是预期结果。"

READING_TITLE = "阅读样本：三图有序"
READING_INTRO = "本页供 TailSync R4 实机粘贴对照使用：正文与图片均为公开自造测试内容，不含任何第三方版权素材。"
READING_EXPECTATION = "预期：直接粘贴与历史恢复粘贴都应得到同一顺序——先正文，再 PNG 图一，再正文，再 JPEG 图二，再正文，再静态 WebP 图三。"
READING_PARAGRAPHS = (
    "第一段正文：本段应出现在 PNG 图一之后、JPEG 图二之前。",
    "第二段正文：本段应出现在 JPEG 图二之后、静态 WebP 图三之前。",
    "第三段正文：本段应出现在静态 WebP 图三之后，本页正文到此结束。",
)

READING_STYLE_LINES = (
    "body { margin: 24px auto; max-width: 720px; padding: 0 16px; font-family: -apple-system, 'PingFang SC', 'Microsoft YaHei', 'Noto Sans CJK SC', sans-serif; line-height: 1.7; color: #1b1b1b; background: #ffffff; }",
    "h1 { font-size: 22px; margin: 0 0 12px; }",
    "p { margin: 12px 0; }",
    "figure { margin: 18px 0; }",
    "img { display: block; width: 120px; height: 90px; object-fit: contain; border: 1px solid #c8c8c8; background-color: #efefef; background-image: linear-gradient(45deg, #dcdcdc 25%, transparent 25%, transparent 75%, #dcdcdc 75%), linear-gradient(45deg, #dcdcdc 25%, transparent 25%, transparent 75%, #dcdcdc 75%); background-size: 12px 12px; background-position: 0 0, 6px 6px; }",
    "figcaption { margin-top: 6px; font-size: 13px; color: #555555; }",
)

STYLES_TITLE = "样式样本：语义标签与等效内联 CSS"
STYLES_INTRO = "本页供 TailSync R4 实机粘贴对照使用：语义标签与等效内联 CSS 各写一遍，均为公开自造测试内容。"
STYLES_HEADING_THREE = "三级标题：标题层级最多到三级"
STYLES_QUOTE_ONE = "第一层引用：粘贴后应保留引用层级。"
STYLES_QUOTE_TWO = "第二层引用：外层段落。"
STYLES_QUOTE_THREE = "第三层引用：嵌套层级不应丢失。"
STYLES_INLINE_CODE = "tailsync --preview"
STYLES_CODE_LINES = (
    "第一行：无缩进",
    "  第二行：缩进两格",
    "    第三行：缩进四格",
    "",
    "空行之后是最后一行：缩进与空行都必须保留",
)
STYLES_UNORDERED_ONE = "无序列表第一项"
STYLES_UNORDERED_TWO = "无序列表第二项"
STYLES_UNORDERED_NESTED_ONE = "嵌套无序项 A"
STYLES_UNORDERED_NESTED_TWO = "嵌套无序项 B"
STYLES_ORDERED_ONE = "有序列表第一项"
STYLES_ORDERED_TWO = "有序列表第二项"
STYLES_ORDERED_NESTED_ONE = "嵌套有序项 A"
STYLES_ORDERED_NESTED_TWO = "嵌套有序项 B"
STYLES_BOLD = "加粗"
STYLES_ITALIC = "倾斜"
STYLES_UNDERLINE = "下划线"
STYLES_STRIKETHROUGH = "删除线"
STYLES_MARK = "语义高亮 mark"
STYLES_HIGHLIGHT_SPAN = "内联背景色高亮"

STYLES_TEXT = (
    STYLES_TITLE,
    STYLES_INTRO,
    "标题",
    STYLES_HEADING_THREE,
    "引用",
    STYLES_QUOTE_ONE,
    STYLES_QUOTE_TWO,
    STYLES_QUOTE_THREE,
    "代码",
    "行内代码：tailsync --preview 应保持等宽。",
    "第一行：无缩进",
    "  第二行：缩进两格",
    "    第三行：缩进四格",
    "空行之后是最后一行：缩进与空行都必须保留",
    "列表",
    STYLES_UNORDERED_ONE,
    STYLES_UNORDERED_TWO,
    STYLES_UNORDERED_NESTED_ONE,
    STYLES_UNORDERED_NESTED_TWO,
    STYLES_ORDERED_ONE,
    STYLES_ORDERED_TWO,
    STYLES_ORDERED_NESTED_ONE,
    STYLES_ORDERED_NESTED_TWO,
    "强调与高亮",
    "基础强调：加粗、倾斜、下划线、删除线、H2O 下标、x2 上标。",
    "颜色高亮：语义高亮 mark，等效内联 CSS：内联背景色高亮。",
    "等效内联 CSS 写法示例：加粗、倾斜、下划线、删除线。",
    "链接",
    "安全 HTTP(S) 链接：阅读样本",
    "非 HTTP(S) 链接：邮件示例（预览应显示文字而非可打开链接）",
    "简单表格",
    "项目",
    "内容",
    "公开样本",
    "简单表格",
    "显式颜色：红色文字；低对比度高亮文字（仅预览调整对比度）。",
)

STYLES_STYLE_LINES = (
    "body { margin: 24px auto; max-width: 720px; padding: 0 16px; font-family: -apple-system, 'PingFang SC', 'Microsoft YaHei', 'Noto Sans CJK SC', sans-serif; line-height: 1.7; color: #1b1b1b; background: #ffffff; }",
    "h1 { font-size: 22px; margin: 0 0 12px; }",
    "h2 { font-size: 18px; margin: 24px 0 8px; }",
    "h3 { font-size: 16px; margin: 16px 0 8px; }",
    "p { margin: 12px 0; }",
    "blockquote { margin: 12px 0 12px 16px; padding: 4px 12px; border-left: 3px solid #b0b0b0; color: #3d3d3d; }",
    "code { font-family: 'SFMono-Regular', Menlo, Consolas, 'Courier New', monospace; background: #f2f2f2; padding: 1px 4px; border-radius: 3px; }",
    "pre { font-family: 'SFMono-Regular', Menlo, Consolas, 'Courier New', monospace; background: #f6f6f6; border: 1px solid #e0e0e0; padding: 10px 12px; overflow-x: auto; }",
    "ul, ol { margin: 12px 0; padding-left: 28px; }",
    "mark { background-color: #ffe58f; }",
)

PARTIAL_TITLE = "阅读样本：缺图（预期失败对照）"
PARTIAL_INTRO = "本页故意只随目录发布 PNG 与静态 WebP 两张图；第二张 JPEG 不发布，用于确认缺失资源不会被静默替换成其它图片。"
PARTIAL_EXPECTATION = "预期：粘贴后应看到图一 PNG 与图三静态 WebP，第二张图缺失或显示明确的缺失状态；不得用图一或图三顶替第二张的位置。"
PARTIAL_FAILURE_NOTE = "如果三张图都出现，或第二张被替换成其它图片，说明缺失资源被错误补齐，应记为失败。"

RTF_TITLE = "TailSync R4 编辑器样本"
RTF_SUBTITLE = "六种基础强调（加粗、倾斜、下划线、删除线、下标、上标）供真实编辑器打开并复制。"
RTF_BOLD_LINE = "加粗示例：这一行应为加粗。"
RTF_ITALIC_LINE = "倾斜示例：这一行应为斜体。"
RTF_UNDERLINE_LINE = "下划线示例：这一行应带下划线。"
RTF_STRIKETHROUGH_LINE = "删除线示例：这一行应带删除线。"
RTF_SUBSCRIPT_PREFIX = "下标示例：H"
RTF_SUBSCRIPT_TEXT = "2"
RTF_SUBSCRIPT_SUFFIX = "O 的 2 应为下标。"
RTF_SUPERSCRIPT_PREFIX = "上标示例：x"
RTF_SUPERSCRIPT_TEXT = "2"
RTF_SUPERSCRIPT_SUFFIX = " 的 2 应为上标。"

EMPHASIS_ORDER = ("bold", "italic", "underline", "strikethrough", "subscript", "superscript")

RTF_TEXT_LINES = (
    RTF_TITLE,
    RTF_SUBTITLE,
    RTF_BOLD_LINE,
    RTF_ITALIC_LINE,
    RTF_UNDERLINE_LINE,
    RTF_STRIKETHROUGH_LINE,
    RTF_SUBSCRIPT_PREFIX + RTF_SUBSCRIPT_TEXT + RTF_SUBSCRIPT_SUFFIX,
    RTF_SUPERSCRIPT_PREFIX + RTF_SUPERSCRIPT_TEXT + RTF_SUPERSCRIPT_SUFFIX,
)

DEPLOYMENT_NOTES = {
    "documentation_default": "未部署：使用文档保留域 example.com，不可用于实机验收；请用 --public-base 指定真实公共基址后再由操作者发布。",
    "cli_argument": "工具不发布、不启动 HTTP 服务、不下载；需操作者自行把本目录发布到该基址后，才可能用于实机验收。",
}

ACCEPTANCE_NOTE = "真实浏览器与两个实际编辑应用的同目标直接/历史成对粘贴尚未执行，本目录及其 manifest 不得作为实机验收证据。"


class FixtureError(Exception):
    pass


def _sha256_bytes(payload: bytes) -> str:
    digest = hashlib.sha256()
    digest.update(payload)
    return digest.hexdigest()


def _image_path(image: dict) -> str:
    return f"{IMAGE_DIRECTORY}/{image['name']}"


def _reject_non_public_host(host: str) -> None:
    lowered = host.lower().rstrip(".")
    if not lowered:
        raise FixtureError("--public-base 缺少主机名")
    if lowered == "localhost" or lowered.endswith(".localhost"):
        raise FixtureError("--public-base 拒绝本机地址 localhost")
    for suffix in PRIVATE_HOST_SUFFIXES:
        if lowered.endswith(suffix):
            raise FixtureError(f"--public-base 拒绝本地/私网主机后缀 {suffix}")
    try:
        address = ipaddress.ip_address(lowered)
    except ValueError:
        address = None
    if address is not None:
        mapped = getattr(address, "ipv4_mapped", None)
        if not address.is_global or address.is_multicast or address.is_reserved or (mapped is not None and (not mapped.is_global or mapped.is_multicast or mapped.is_reserved)):
            raise FixtureError(f"--public-base 拒绝非公网地址 {address}")
        return
    labels = lowered.split(".")
    if len(labels) < 2 or not re.fullmatch(r"[a-z][a-z0-9-]*", labels[-1]):
        raise FixtureError("--public-base 需要可公开解析的完整主机名")
    if len(lowered) > 253 or any(len(label) > 63 or not re.fullmatch(r"[a-z0-9](?:[a-z0-9-]*[a-z0-9])?", label) for label in labels):
        raise FixtureError("--public-base 主机名格式无效")


def normalize_public_base(raw: str) -> str:
    if raw is None:
        raise FixtureError("--public-base 不能为空")
    text = raw
    if not text:
        raise FixtureError("--public-base 不能为空")
    for char in text:
        if char.isspace() or ord(char) < 0x20 or 0x7F <= ord(char) <= 0x9F or char == "\\":
            raise FixtureError("--public-base 不允许空白或控制字符")
    try:
        parts = urlsplit(text)
        host = parts.hostname
        port = parts.port
    except ValueError as error:
        raise FixtureError(f"--public-base URL 无效：{error}") from error
    if parts.scheme not in ("http", "https"):
        raise FixtureError("--public-base 只接受 http 或 https 基址")
    if "@" in parts.netloc:
        raise FixtureError("--public-base 不允许携带凭据")
    if "?" in text or "#" in text:
        raise FixtureError("--public-base 不允许查询串或片段")
    if port is not None and not 1 <= port <= 65535:
        raise FixtureError("--public-base 端口必须在 1 到 65535 之间")
    if not host:
        raise FixtureError("--public-base 缺少主机名")
    if "%" in host:
        raise FixtureError("--public-base 主机名不允许转义或 zone identifier")
    if ":" not in host:
        try:
            host = host.encode("idna").decode("ascii").lower()
        except UnicodeError as error:
            raise FixtureError("--public-base 主机名编码无效") from error
    _reject_non_public_host(host)
    path = parts.path or "/"
    if not path.startswith("/"):
        path = "/" + path
    for segment in unquote(path).split("/"):
        if segment in (".", ".."):
            raise FixtureError("--public-base 路径不允许 . 或 .. 段")
    if not path.endswith("/"):
        path = path + "/"
    netloc = f"[{host}]" if ":" in host else host
    if port is not None:
        netloc += f":{port}"
    return urlunsplit((parts.scheme, netloc, path, "", ""))


def _rtf_escape(text: str) -> str:
    chunks = []
    for char in text:
        code = ord(char)
        if char == "\\":
            chunks.append("\\\\")
        elif char == "{":
            chunks.append("\\{")
        elif char == "}":
            chunks.append("\\}")
        elif 0x20 <= code < 0x7F:
            chunks.append(char)
        elif code > 0xFFFF:
            for unit in (0xD800 + ((code - 0x10000) >> 10), 0xDC00 + ((code - 0x10000) & 0x3FF)):
                chunks.append(f"\\u{unit - 0x10000}?")
        else:
            signed = code if code < 0x8000 else code - 0x10000
            chunks.append(f"\\u{signed}?")
    return "".join(chunks)


def _document_head(title: str, style_lines: tuple) -> list:
    lines = [
        "<!DOCTYPE html>",
        '<html lang="zh-CN">',
        "<head>",
        '<meta charset="utf-8">',
        '<meta name="viewport" content="width=device-width, initial-scale=1">',
        f"<title>{title}</title>",
        "<style>",
    ]
    lines.extend(style_lines)
    lines.extend(["</style>", "</head>", "<body>"])
    return lines


def render_reading_html() -> str:
    lines = _document_head(READING_TITLE, READING_STYLE_LINES)
    lines.extend([
        f"<h1>{READING_TITLE}</h1>",
        f"<p>{READING_INTRO}</p>",
        f"<p>{READING_EXPECTATION}</p>",
    ])
    for index, image in enumerate(IMAGE_SOURCES):
        lines.extend([
            "<figure>",
            f'<img src="{_image_path(image)}" alt="{image["alt"]}">',
            f"<figcaption>{image['caption']}</figcaption>",
            "</figure>",
            f"<p>{READING_PARAGRAPHS[index]}</p>",
        ])
    lines.extend(["</body>", "</html>", ""])
    return "\n".join(lines)


def render_styles_html(public_base: str = DOCUMENTATION_BASE) -> str:
    lines = _document_head(STYLES_TITLE, STYLES_STYLE_LINES)
    lines.extend([
        f"<h1>{STYLES_TITLE}</h1>",
        f"<p>{STYLES_INTRO}</p>",
        "<h2>标题</h2>",
        f"<h3>{STYLES_HEADING_THREE}</h3>",
        "<h2>引用</h2>",
        f"<blockquote>{STYLES_QUOTE_ONE}</blockquote>",
        "<blockquote>",
        f"<p>{STYLES_QUOTE_TWO}</p>",
        f"<blockquote>{STYLES_QUOTE_THREE}</blockquote>",
        "</blockquote>",
        "<h2>代码</h2>",
        f"<p>行内代码：<code>{STYLES_INLINE_CODE}</code> 应保持等宽。</p>",
        "<pre><code>" + "\n".join(STYLES_CODE_LINES) + "</code></pre>",
        "<h2>列表</h2>",
        "<ul>",
        f"<li>{STYLES_UNORDERED_ONE}</li>",
        f"<li>{STYLES_UNORDERED_TWO}",
        "<ul>",
        f"<li>{STYLES_UNORDERED_NESTED_ONE}</li>",
        f"<li>{STYLES_UNORDERED_NESTED_TWO}</li>",
        "</ul>",
        "</li>",
        "</ul>",
        "<ol>",
        f"<li>{STYLES_ORDERED_ONE}</li>",
        f"<li>{STYLES_ORDERED_TWO}",
        "<ol>",
        f"<li>{STYLES_ORDERED_NESTED_ONE}</li>",
        f"<li>{STYLES_ORDERED_NESTED_TWO}</li>",
        "</ol>",
        "</li>",
        "</ol>",
        "<h2>强调与高亮</h2>",
        f"<p>基础强调：<strong>{STYLES_BOLD}</strong>、<em>{STYLES_ITALIC}</em>、<u>{STYLES_UNDERLINE}</u>、<s>{STYLES_STRIKETHROUGH}</s>、H<sub>2</sub>O 下标、x<sup>2</sup> 上标。</p>",
        f'<p>颜色高亮：<mark>{STYLES_MARK}</mark>，等效内联 CSS：<span style="background-color: #ffe58f;">{STYLES_HIGHLIGHT_SPAN}</span>。</p>',
        f'<p>等效内联 CSS 写法示例：<span style="font-weight: 700;">{STYLES_BOLD}</span>、<span style="font-style: italic;">{STYLES_ITALIC}</span>、<span style="text-decoration: underline;">{STYLES_UNDERLINE}</span>、<span style="text-decoration: line-through;">{STYLES_STRIKETHROUGH}</span>。</p>',
        "<h2>链接</h2>",
        f'<p>安全 HTTP(S) 链接：<a href="{html.escape(public_base + "reading.html", quote=True)}">阅读样本</a></p>',
        '<p>非 HTTP(S) 链接：<a href="mailto:tailsync-r4@example.com">邮件示例（预览应显示文字而非可打开链接）</a></p>',
        "<h2>简单表格</h2>",
        "<table>",
        "<tr><th>项目</th><th>内容</th></tr>",
        "<tr><td>公开样本</td><td>简单表格</td></tr>",
        "</table>",
        '<p>显式颜色：<span style="color: #b91c1c;">红色文字</span>；<span style="color: #222222; background-color: #222222;">低对比度高亮文字（仅预览调整对比度）</span>。</p>',
    ])
    lines.extend(["</body>", "</html>", ""])
    return "\n".join(lines)


def render_partial_html() -> str:
    lines = _document_head(PARTIAL_TITLE, READING_STYLE_LINES)
    lines.extend([
        f"<h1>{PARTIAL_TITLE}</h1>",
        f"<p>{PARTIAL_INTRO}</p>",
        f"<p>{PARTIAL_EXPECTATION}</p>",
        "<figure>",
        f'<img src="{_image_path(IMAGE_SOURCES[0])}" alt="{IMAGE_SOURCES[0]["alt"]}">',
        f"<figcaption>{IMAGE_SOURCES[0]['caption']}</figcaption>",
        "</figure>",
        "<figure>",
        f'<img src="{MISSING_IMAGE_PATH}" alt="{MISSING_IMAGE_ALT}">',
        f"<figcaption>{MISSING_IMAGE_CAPTION}</figcaption>",
        "</figure>",
        "<figure>",
        f'<img src="{_image_path(IMAGE_SOURCES[2])}" alt="{IMAGE_SOURCES[2]["alt"]}">',
        f"<figcaption>{IMAGE_SOURCES[2]['caption']}</figcaption>",
        "</figure>",
        f"<p>{PARTIAL_FAILURE_NOTE}</p>",
    ])
    lines.extend(["</body>", "</html>", ""])
    return "\n".join(lines)


def render_editor_rtf() -> str:
    lines = [
        "{\\rtf1\\ansi\\ansicpg1252\\deff0\\uc1",
        "{\\fonttbl{\\f0\\fswiss\\fcharset0 Helvetica;}{\\f1\\fmodern\\fcharset0 Courier New;}}",
        "\\viewkind4\\pard\\lang2052\\f0\\fs24",
        "\\pard " + _rtf_escape(RTF_TITLE) + "\\par",
        "\\pard " + _rtf_escape(RTF_SUBTITLE) + "\\par",
        "\\pard {\\b " + _rtf_escape(RTF_BOLD_LINE) + "}\\par",
        "\\pard {\\i " + _rtf_escape(RTF_ITALIC_LINE) + "}\\par",
        "\\pard {\\ul " + _rtf_escape(RTF_UNDERLINE_LINE) + "}\\par",
        "\\pard {\\strike " + _rtf_escape(RTF_STRIKETHROUGH_LINE) + "}\\par",
        "\\pard " + _rtf_escape(RTF_SUBSCRIPT_PREFIX) + "{\\sub " + _rtf_escape(RTF_SUBSCRIPT_TEXT) + "}" + _rtf_escape(RTF_SUBSCRIPT_SUFFIX) + "\\par",
        "\\pard " + _rtf_escape(RTF_SUPERSCRIPT_PREFIX) + "{\\sup " + _rtf_escape(RTF_SUPERSCRIPT_TEXT) + "}" + _rtf_escape(RTF_SUPERSCRIPT_SUFFIX) + "\\par",
        "}",
        "",
    ]
    return "\n".join(lines)


def reading_expectation(public_base: str) -> dict:
    images = []
    text = [READING_TITLE, READING_INTRO, READING_EXPECTATION]
    for index, image in enumerate(IMAGE_SOURCES):
        relative = _image_path(image)
        images.append({
            "order": index + 1,
            "encoding": image["encoding"],
            "path": relative,
            "url": public_base + relative,
            "present": True,
            "source": image["source"],
        })
        text.append(image["caption"])
        text.append(READING_PARAGRAPHS[index])
    return {
        "title": READING_TITLE,
        "text": text,
        "image_count": len(images),
        "image_encodings": [image["encoding"] for image in IMAGE_SOURCES],
        "images": images,
    }


def styles_expectation(public_base: str = DOCUMENTATION_BASE) -> dict:
    return {
        "title": STYLES_TITLE,
        "text": list(STYLES_TEXT),
        "image_count": 0,
        "image_encodings": [],
        "images": [],
        "semantics": ["heading", "quote", "code", "code_block", "list", "highlight", "emphasis", "link", "table", "color"],
        "links": [
            {"target": public_base + "reading.html", "clickable_in_preview": True, "automatic_request": False},
            {"target": "mailto:tailsync-r4@example.com", "clickable_in_preview": False, "automatic_request": False},
        ],
        "table_cells": [["项目", "内容"], ["公开样本", "简单表格"]],
        "inline_css": [
            "font-weight: 700",
            "font-style: italic",
            "text-decoration: underline",
            "text-decoration: line-through",
            "background-color: #ffe58f",
        ],
    }


def partial_expectation(public_base: str) -> dict:
    images = [
        {
            "order": 1,
            "encoding": IMAGE_SOURCES[0]["encoding"],
            "path": _image_path(IMAGE_SOURCES[0]),
            "url": public_base + _image_path(IMAGE_SOURCES[0]),
            "present": True,
            "source": IMAGE_SOURCES[0]["source"],
        },
        {
            "order": 2,
            "encoding": MISSING_IMAGE_ENCODING,
            "path": MISSING_IMAGE_PATH,
            "url": public_base + MISSING_IMAGE_PATH,
            "present": False,
            "source": None,
        },
        {
            "order": 3,
            "encoding": IMAGE_SOURCES[2]["encoding"],
            "path": _image_path(IMAGE_SOURCES[2]),
            "url": public_base + _image_path(IMAGE_SOURCES[2]),
            "present": True,
            "source": IMAGE_SOURCES[2]["source"],
        },
    ]
    return {
        "title": PARTIAL_TITLE,
        "text": [
            PARTIAL_TITLE,
            PARTIAL_INTRO,
            PARTIAL_EXPECTATION,
            IMAGE_SOURCES[0]["caption"],
            MISSING_IMAGE_CAPTION,
            IMAGE_SOURCES[2]["caption"],
            PARTIAL_FAILURE_NOTE,
        ],
        "image_count": 3,
        "image_encodings": [image["encoding"] for image in images],
        "present_image_count": 2,
        "missing_image_count": 1,
        "images": images,
    }


def rtf_expectation() -> dict:
    return {
        "title": RTF_TITLE,
        "text": list(RTF_TEXT_LINES),
        "image_count": 0,
        "image_encodings": [],
        "images": [],
        "emphasis_count": len(EMPHASIS_ORDER),
        "emphasis_order": list(EMPHASIS_ORDER),
    }


def _document_entry(payload: bytes, role: str, content_type: str, expected: dict) -> dict:
    return {
        "sha256": _sha256_bytes(payload),
        "bytes": len(payload),
        "role": role,
        "content_type": content_type,
        "acceptance_status": UNKNOWN,
        "expected": expected,
    }


def generate(output_dir, public_base_raw=None) -> dict:
    output_path = Path(output_dir)
    if not output_path.is_absolute():
        raise FixtureError("--output 必须是绝对目录")
    if os.path.lexists(output_path):
        raise FixtureError(f"--output 已存在，拒绝覆盖：{output_path}")
    if public_base_raw is None:
        public_base = DOCUMENTATION_BASE
        public_base_origin = "documentation_default"
    else:
        public_base = normalize_public_base(public_base_raw)
        public_base_origin = "cli_argument"

    payloads = {
        "reading.html": render_reading_html().encode("utf-8"),
        "styles.html": render_styles_html(public_base).encode("utf-8"),
        "partial.html": render_partial_html().encode("utf-8"),
        "editor.rtf": render_editor_rtf().encode("ascii"),
    }

    source_payloads = {}
    source_images = []
    for image in IMAGE_SOURCES:
        source_path = REPO_ROOT / image["source"]
        if not source_path.is_file():
            raise FixtureError(f"缺少只读原图：{source_path}")
        payload = source_path.read_bytes()
        source_payloads[image["name"]] = payload
        payloads[_image_path(image)] = payload
        source_images.append({
            "name": image["name"],
            "encoding": image["encoding"],
            "content_type": image["content_type"],
            "source": image["source"],
            "source_sha256": _sha256_bytes(payload),
            "bytes": len(payload),
        })

    try:
        output_path.mkdir(parents=True, exist_ok=False)
    except FileExistsError as error:
        raise FixtureError(f"--output 已存在，拒绝覆盖：{output_path}") from error
    for relative, payload in payloads.items():
        target = output_path / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(payload)

    digests = {}
    for relative, payload in payloads.items():
        written = (output_path / relative).read_bytes()
        if written != payload:
            raise FixtureError(f"写入校验失败：{relative}")
        digests[relative] = _sha256_bytes(written)

    entries = {
        "reading.html": _document_entry(payloads["reading.html"], "reading", "text/html; charset=utf-8", reading_expectation(public_base)),
        "styles.html": _document_entry(payloads["styles.html"], "styles", "text/html; charset=utf-8", styles_expectation(public_base)),
        "partial.html": _document_entry(payloads["partial.html"], "partial", "text/html; charset=utf-8", partial_expectation(public_base)),
        "editor.rtf": _document_entry(payloads["editor.rtf"], "rtf", "text/rtf", rtf_expectation()),
    }
    for image in IMAGE_SOURCES:
        relative = _image_path(image)
        entries[relative] = {
            "sha256": digests[relative],
            "bytes": len(payloads[relative]),
            "role": "image",
            "content_type": image["content_type"],
            "encoding": image["encoding"],
            "source": image["source"],
            "source_sha256": _sha256_bytes(source_payloads[image["name"]]),
            "unchanged_from_source": payloads[relative] == source_payloads[image["name"]],
            "acceptance_status": UNKNOWN,
        }

    manifest = {
        "schema": SCHEMA,
        "schema_version": SCHEMA_VERSION,
        "task_id": TASK_ID,
        "generator": GENERATOR,
        "public_base": public_base,
        "deployment": {
            "public_base": public_base,
            "public_base_origin": public_base_origin,
            "published_by_tool": False,
            "manual_publish_required": True,
            "acceptance_eligible": False,
            "network_verified": False,
            "publication_status": UNKNOWN,
            "note": DEPLOYMENT_NOTES[public_base_origin],
        },
        "acceptance": {
            "status": UNKNOWN,
            "real_device_tested": False,
            "note": ACCEPTANCE_NOTE,
        },
        "source_images": source_images,
        "files": entries,
    }

    manifest_payload = (json.dumps(manifest, ensure_ascii=False, indent=2) + "\n").encode("utf-8")
    manifest_path = output_path / MANIFEST_NAME
    manifest_path.write_bytes(manifest_payload)
    if manifest_path.read_bytes() != manifest_payload:
        raise FixtureError(f"写入校验失败：{MANIFEST_NAME}")

    return {
        "output_dir": str(output_path),
        "manifest_path": str(manifest_path),
        "files": entries,
        "manifest": manifest,
    }


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="rich-text-r4-fixtures.py",
        description="生成 TailSync R4 富文本实机粘贴样本目录；只写文件，不发布、不启动服务、不联网。",
    )
    parser.add_argument("--output", required=True, metavar="绝对目录", help="输出目录，必须为绝对路径且尚不存在")
    parser.add_argument("--public-base", dest="public_base", default=None, metavar="URL", help="无凭据的 http(s) 公共基址；省略时使用文档保留域 example.com 并在 manifest 标记未部署")
    return parser


def main(argv=None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        result = generate(args.output, args.public_base)
    except FixtureError as error:
        print(f"错误：{error}", file=sys.stderr)
        return EXIT_USAGE
    except OSError as error:
        print(f"错误：写入失败：{error}", file=sys.stderr)
        return EXIT_FAILURE
    print(f"已生成 {len(result['files']) + 1} 个文件：{result['output_dir']}")
    print(f"manifest：{result['manifest_path']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
