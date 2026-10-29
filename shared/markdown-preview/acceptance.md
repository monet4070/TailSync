# Markdown 阅读预览

清晰的正文、稳定的表格和离线公式。**重点内容**、*补充说明*和 `inline code` 应当保持自然的阅读节奏。

## 表格：对齐、换行与长内容

| 项目 | 说明 | 数值 | 公式 |
| :--- | :--- | ---: | :---: |
| 中文排版 | 较长的中文说明应当在单元格内正常换行，不挤占整个预览窗口，也不盖住相邻列。 | 1,024 | $E=mc^2$ |
| 转义竖线 | `a\|b` 与普通文本 A\|B 都只有一个单元格。 | 42 | $\frac{1}{2}$ |
| 长链接 | https://example.com/a/very/long/path/that/should/wrap/within/the/cell | 3.14 | $\sqrt{x^2+y^2}$ |

## 公式

行内公式 $a^2+b^2=c^2$ 和 \(\alpha+\beta=\gamma\) 与文字处于同一行。

$$
\int_0^1 x^2\,dx=\frac{1}{3}
\qquad
\begin{pmatrix}a&b\\c&d\end{pmatrix}
$$

\[
f(x)=\sum_{n=0}^{\infty}\frac{x^n}{n!}
\]

## 列表与代码

1. 有序列表的第一项。
   - 嵌套列表应当正确缩进。
   - 支持 **加粗**、`代码` 和 $x_i$。
2. 第二项保留正确的序号。

- [x] 已完成的任务
- [ ] 尚未完成的任务

> 引用内容有独立的间距和边线。
>
> 多段引用仍然属于同一组内容。

```python
# 代码中的 $x^2$ 不应被当作公式
def normalize(value):
    return value / 1024
```

价格 $5 和 $10 保持原样。非法公式 $\unknowncommand{x}$ 保留可读源码，不影响后面的文字。
